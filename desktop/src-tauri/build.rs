fn main() {
  // Ensure a minimal valid icon.png exists to satisfy tauri::generate_context! icon reading
  let icon_png = std::path::Path::new("icons/icon.png");
  let _ = std::fs::create_dir_all("icons");
  // Create a 1x1 transparent RGBA PNG
  {
    use image::{ImageBuffer, Rgba};
    let img: ImageBuffer<Rgba<u8>, Vec<u8>> = ImageBuffer::from_fn(1, 1, |_x, _y| Rgba([0, 0, 0, 0]));
    if let Err(e) = img.save(icon_png) {
      println!("cargo:warning=failed to write RGBA icon.png: {}", e);
    }
  }

  // Ensure .ico exists (Windows RC) — if missing, generate a tiny transparent icon
  let icon_ico = std::path::Path::new("icons/icon.ico");
  if !icon_ico.exists() {
    println!("cargo:warning=icons/icon.ico not found; generating minimal transparent .ico");
    if let Err(e) = (|| -> Result<(), Box<dyn std::error::Error>> {
      use std::fs::File;
      use ico::{IconDir, IconImage, IconDirEntry, ResourceType};
      // 64x64 transparent RGBA
      let w = 64u32; let h = 64u32; let data = vec![0u8; (w * h * 4) as usize];
      let img = IconImage::from_rgba_data(w, h, data);
      let mut dir = IconDir::new(ResourceType::Icon);
      dir.add_entry(IconDirEntry::encode(&img)?);
      let mut f = File::create(icon_ico)?;
      dir.write(&mut f)?;
      Ok(())
    })() {
      println!("cargo:warning=failed to generate icons/icon.ico: {}", e);
    }
  }

  // Always run tauri_build to generate tauri.conf bindings/context
  tauri_build::build();
}
