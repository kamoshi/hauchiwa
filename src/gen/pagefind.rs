use std::process::Command;

use camino::Utf8Path;

pub(crate) fn build_pagefind(out: &Utf8Path) {
	let res = Command::new("pagefind")
		.args(["--site", out.as_str()])
		.output()
		.unwrap();

	println!("{}", String::from_utf8(res.stdout).unwrap());
}
