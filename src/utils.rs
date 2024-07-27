
pub(crate) fn hex(bytes: &[u8]) -> String {
	use std::fmt::Write;
	let mut acc = String::with_capacity(bytes.len() * 2);

	for byte in bytes {
		write!(&mut acc, "{:02x}", byte).unwrap();
	}

	acc
}
