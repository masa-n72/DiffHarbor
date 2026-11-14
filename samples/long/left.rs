fn main() {
	let config = load_config("settings.toml");
	if config.verbose {
		println!("とても長い行のテストです。とても長い行のテストです。とても長い行のテストです。とても長い行のテストです。とても長い行のテストです。とても長い行のテストです。とても長い行のテストです。とても長い行のテストです。とても長い行のテストです。とても長い行のテストです。とても長い行のテストです。とても長い行のテストです。終わり");
	}
	let total = items.iter().map(|item| item.price * item.count).sum::<u64>();
	return total;
}
