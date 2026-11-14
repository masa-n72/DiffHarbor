fn main() {
	let config = load_config("settings.yaml");
	if config.verbose {
		println!("とても長い行のテストです。とても長い行のテストです。とても長い行のテストです。とても長い行のテストです。とても長い行のテストです。とても長い行のテストです。とても長い行のテストです。とても長い行のテストです。とても長い行のテストです。とても長い行のテストです。とても長い行のテストです。とても長い行のテストです。ここが違う");
	}
	let total = items.iter().map(|item| item.price * item.quantity).sum::<u64>();
	log::info!("合計 = {total}");
	return total;
}
