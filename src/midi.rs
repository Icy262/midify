pub(crate) struct Note {
	pub(crate) midi_num: u8,
	pub(crate) start_frame: usize,
	pub(crate) end_frame: usize,
}

impl Note {
	pub(crate) fn freq_to_midi_num(freq: f32) -> u8 {
		69 + 12*((freq/440.0f32).log2().round() as u8)
	}

	pub(crate) fn is_freq(&self, freq: f32) -> bool {
		self.midi_num == Note::freq_to_midi_num(freq)
	}
}