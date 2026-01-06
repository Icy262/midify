use flac::StreamReader;
use std::fs::File;
use realfft::RealFftPlanner;

fn main() {
	//get the path to the flac
	println!("Enter the path to the flac:");
	let mut path = String::new();
	std::io::stdin().read_line(&mut path).unwrap();
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
	let hann_window: Vec<f32> = (0..window_size).map(|n| {0.5 - (2.0f32*std::f32::consts::PI*(n as f32)/((window_size - 1) as f32)).cos()/2.0f32}).collect();

	//calculate frequency of each bin. freq is the top end of the bin
	let freqs: Vec<f32> = (0..=window_size/2).map(|k| (k + 1) as f32 * (sample_rate as f32) / (window_size as f32)).collect(); //Nyquist

	//perform fft on the mono pcm
	let mut planner = RealFftPlanner::<f32>::new();
	let r2c = planner.plan_fft_forward(window_size);
	let mut spectrum = r2c.make_output_vec();
	let total_frames = (mono_flac.len() - window_size)/hop_size + 1;
	let mut in_buffer = vec![0.0f32; window_size];
	let mut output = vec![vec![0.0f32; freqs.len()]; total_frames];
	for current_frame in 0..total_frames {
		let frame_start = current_frame * hop_size;
		let frame_end = frame_start + window_size;
		
		//windowing
		for i in 0..window_size {
			in_buffer[i] = mono_flac[frame_start + i] * hann_window[i];
		}

		r2c.process(&mut in_buffer, &mut spectrum).expect("fft failed");

		//represent loudness as a fraction of the RMS
		for (frequency_bin, magnitude) in spectrum.iter().enumerate() {
			output[current_frame][frequency_bin] = (magnitude.re.powi(2) + magnitude.im.powi(2)).sqrt();
		}
	}
	println!("FFT complete");
}