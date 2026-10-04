fn main() {
    let out = std::path::Path::new("../town-raw");
    std::fs::create_dir_all(out).unwrap();
    for file in ["SSCCHR1.R3", "SSCCHR2.R3", "MARK.R3"] {
        let data = std::fs::read(format!("../original-chinese/{file}")).unwrap();
        let a = hero_import::ls11::Archive::parse(&data).unwrap();
        for i in 0..a.len() {
            std::fs::write(out.join(format!("{file}-{i:03}.bin")), a.decode(i).unwrap()).unwrap();
        }
    }
}
