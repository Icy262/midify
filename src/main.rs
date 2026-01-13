mod midi;

use flac::StreamReader;
use midly::TrackEvent;
use std::fs::File;
use realfft::RealFftPlanner;
use crate::midi::Note;
use midly::Track;
use midly::TrackEventKind;
use midly::MidiMessage;
use midly::num::u4;
use midly::num::u7;
use midly::num::u15;
use midly::num::u28;
use midly::Header;
use midly::Timing;
use midly::Format;
use midly::Smf;

fn main() {
	//get the path to the flac
	println!("Enter the path to the flac:");
	let mut path = String::new();
	std::io::stdin()
		.read_line(&mut path)
		.unwrap();
	let path = path.trim();

	//very small hop size to maximize the time resolution
	let window_size = 4096;
	let hop_size = 256;

	//load the flac
	let mut flac = StreamReader::<File>::from_file(path).expect("File read failed");

	
	//get info about the flac
	let channels = flac.info().channels; //eg. 2, left and right
	let sample_rate = flac.info().sample_rate; //eg. 44.1 khz

	//normalize amplitudes to within -1 to 1
	let norm = 1.0f32/32768.0f32; //32768 is 2^16. the sample will be i16, meaning between -2^16 and 2^16.

	//combine all channels to mono
	let mut mono_flac = Vec::<f32>::new(); //flac, but all channels are averaged
	let mut sum_channels = 0.0f32;
	let mut channels_processed = 0u8;
	for sample in flac.iter::<i16>() {
		sum_channels += sample as f32;
		channels_processed += 1;
		
		if channels_processed == channels {
			mono_flac.push(sum_channels*norm/channels as f32);
			sum_channels = 0.0f32;
			channels_processed = 0u8;
		}
	}

	//calculate hann window. length N: w[n] = 0.5 * (1 - cos(2π n / (N-1)))
	let hann_window: Vec<f32> = (0..window_size)
		.map(|n| {
			0.5 - (2.0f32*std::f32::consts::PI*(n as f32)/((window_size - 1) as f32)).cos()/2.0f32
		})
		.collect();

	//calculate frequency of each bin. freq is the top end of the bin
	let freqs: Vec<f32> = (0..=window_size/2)
		.map(|k| (k + 1) as f32 * (sample_rate as f32) / (window_size as f32))
		.collect(); //Nyquist

	//perform fft on the mono pcm
	let mut planner = RealFftPlanner::<f32>::new();
	let r2c = planner.plan_fft_forward(window_size);
	let mut spectrum = r2c.make_output_vec();
	let total_frames = (mono_flac.len() - window_size)/hop_size + 1;
	let mut in_buffer = vec![0.0f32; window_size];
	let mut output = vec![vec![0.0f32; freqs.len()]; total_frames];
	for current_frame in 0..total_frames {	
		//windowing
		for i in 0..window_size {
			in_buffer[i] = mono_flac[current_frame*hop_size + i] * hann_window[i];
		}

		r2c.process(&mut in_buffer, &mut spectrum).expect("fft failed");

		//convert the spectrum to power
		output[current_frame] = spectrum
			.iter()
			.map(|magnitude| {
				magnitude.re.powi(2) + magnitude.im.powi(2)
			})
			.collect();
	}

	println!("FFT complete");

	let mut notes: Vec<Note> = Vec::new();
	let mut current_notes: Vec<Note> = Vec::new();

	//identify fundamentals
	for (current_frame_index, mut frame) in output.into_iter().enumerate() {
		//find total power
		let total_power = frame
			.iter()
			.sum::<f32>();

		//normalize power of each bin to a fraction of total power
		frame = frame.iter()
			.map(|amplitude| amplitude/total_power)
			.collect();

		//find any bin worth more than 10% of total and store index of that bin
		let mut possible_fundamentals: Vec<usize> = frame
			.iter()
			.enumerate()
			.filter_map(|(index, amplitude)| {
				(*amplitude >= 0.10).then_some(index)
			})
			.collect::<Vec<usize>>();
		
		//sort the bin indexes by power
		possible_fundamentals
			.sort_by(|&index_1, &index_2| {
				frame[index_2] //index 1 and 2 are swapped so that the list is high -> low instead of the other way around
					.partial_cmp(&frame[index_1])
					.expect("Amplitude not a positive float")
			});

		let mut confirmed_fundamentals: Vec<usize> = Vec::new();

		//process all possible fundamentals starting with the most powerful
		while possible_fundamentals.len() != 0 {
			let index_of_fundamental = possible_fundamentals[0];

			//remove index of largest power from possible and add to confirmed
			confirmed_fundamentals.push(index_of_fundamental);
			possible_fundamentals.remove(0);

			possible_fundamentals = possible_fundamentals
				.into_iter()
				.filter_map(|index| {
					//remove anything that isn't at least 20% of the strength of the main peak. We can assume it's just a harmonic or sub-harmonic
					(frame[index] >= 0.2f32*frame[index_of_fundamental]).then_some(index)
				})
				.collect();
		}

		//if part of a current note, update the end frame, if not, create a new note.
		for fundamental in confirmed_fundamentals {
			let note_exists = current_notes.iter_mut().find(|note| note.is_freq(freqs[fundamental])); //if the fundamental matches some note in current notes, return some, else none
			if note_exists.is_some() { //note exists
				(note_exists.unwrap()).end_frame = current_frame_index;
			} else { //note doesn't exist
				current_notes.push(Note {
					midi_num: Note::freq_to_midi_num(freqs[fundamental]),
					start_frame: current_frame_index,
					end_frame: current_frame_index
				})
			}
		}

		//if the note hasn't been seen in 10 frames, end the note by transferring to notes
		notes.append(&mut current_notes.extract_if(..,|note| current_frame_index - note.end_frame >= 10).collect::<Vec<Note>>());
	}

	
	let ppq = 480;
	let mut track: Track = Vec::new();

	//convert notes to midi events
	let mut events: Vec<(u32, TrackEventKind)> = Vec::new();
	for note in notes {
		//convert start and end frames to ticks
		let start_tick = note.start_frame*hop_size/sample_rate as usize*2*ppq; //mult by 2 because 2 notes per second (assuming 120bpm)
		let end_tick = note.end_frame*hop_size/sample_rate as usize*2*ppq;

		//push start and end events
		events.push((start_tick as u32, TrackEventKind::Midi {
			channel: u4::from(0),
			message: MidiMessage::NoteOff {
				key: u7::from(note.midi_num),
				vel: u7::from(64), //no support for volume estimation yet, so just normalize all to 64
			}
		}));

		events.push((end_tick as u32, TrackEventKind::Midi {
			channel: u4::from(0),
			message: MidiMessage::NoteOff {
				key: u7::from(note.midi_num),
				vel: u7::from(64),
			}
		}));
	}
	
	//sort events by tick index. If not, note ends would appear immediately after their starts, and before intermediary notes
	events.sort_by_key(|event| event.0);

	let mut previous_tick = 0;
	for (tick, event_kind) in events {
		track.push(TrackEvent {
			delta: u28::from(tick - previous_tick),
			kind: event_kind,
		});
		previous_tick = tick
	}

	//Push end of track
	track.push(TrackEvent {
		delta: u28::from(0),
		kind: TrackEventKind::Meta(midly::MetaMessage::EndOfTrack),
	});

	//create the midi output
	let midi_file = Smf {
		header: Header {
			format: Format::SingleTrack,
			timing: Timing::Metrical(u15::from(ppq as u16)), //convert to u16 then to u15 because usize -> u15 not implemented
		},
		tracks: vec![track],
	};

	//create the output file
	let mut file = File::create(path.replace("flac", "mid")).expect("output file creation failed");
	
	//write the midi output to the output file
	midi_file.write_std(&mut file).expect("mid write failed");
}