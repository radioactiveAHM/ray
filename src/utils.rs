#[inline(always)]
pub const fn convert_two_u8s_to_u16_be(bytes: [u8; 2]) -> u16 {
	((bytes[0] as u16) << 8) | bytes[1] as u16
}

#[inline(always)]
pub const fn convert_u16_to_two_u8s_be(integer: u16) -> [u8; 2] {
	[(integer >> 8) as u8, integer as u8]
}

#[inline(always)]
pub fn catch_in_buff(find: &[u8], buff: &[u8]) -> Option<(usize, usize)> {
	if find.len() >= buff.len() {
		return None;
	}
	buff.windows(find.len())
		.position(|pre| pre == find)
		.map(|a| (a, a + find.len()))
}

/// Short hand of `std::collections::VecDeque<u8>` to use for buffering
pub struct DeqBuffer {
	inner_buf: std::collections::VecDeque<u8>,
}
impl DeqBuffer {
	#[inline(always)]
	pub fn new(capacity: usize) -> Self {
		Self {
			inner_buf: std::collections::VecDeque::with_capacity(capacity),
		}
	}
	#[inline(always)]
	pub fn write(&mut self, buf: &[u8]) {
		self.inner_buf.extend(buf);
	}
	#[inline(always)]
	pub fn slice(&self) -> &[u8] {
		self.inner_buf.as_slices().0
	}
	#[inline(always)]
	pub fn remove(&mut self, len: usize) {
		self.inner_buf.drain(..len);
	}
}
