// cargo run --example snap -- <pdf> <page> <out.png>
fn main() {
    let a: Vec<String> = std::env::args().collect();
    let e = folio_engine::Engine::start(vec!["vendor/pdfium-bundle".into()]).unwrap();
    let p = std::path::PathBuf::from(&a[1]);
    let page: u32 = a[2].parse().unwrap();
    let out = a[3].clone();
    e.exec(move |s| {
        let i = s.open(&p, None)?;
        let img = s.render_image(i.id, page, 1.5, true)?;
        img.save(&out).unwrap();
        Ok(())
    })
    .unwrap();
}
