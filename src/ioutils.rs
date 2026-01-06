pub trait AsyncRecvBytes {
	fn poll_recv_bytes(&mut self, cx: &mut std::task::Context<'_>) -> std::task::Poll<tokio::io::Result<bytes::Bytes>>;
}

#[inline(always)]
pub fn split<T>(xref: &mut T) -> (&mut T, &mut T) {
	unsafe {
		(
			core::mem::transmute::<&mut T, &mut T>(xref),
			core::mem::transmute::<&mut T, &mut T>(xref),
		)
	}
}
