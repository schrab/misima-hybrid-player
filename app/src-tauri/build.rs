use std::io::Write;
use std::process::Command;

fn main() {
    // Generate simple solid icons so tauri-build can succeed without binary assets in git.
    let icons = [
        ("icons/32x32.png", 32u32),
        ("icons/128x128.png", 128u32),
        ("icons/128x128@2x.png", 256u32),
    ];
    for (rel, size) in icons {
        let path = std::path::Path::new(rel);
        if path.exists() {
            continue;
        }
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        write_png(path, size, 10, 28, 32);
    }
    // ico placeholder: copy 32x32 png bytes as fallback (tauri may still want real ico on windows)
    let ico = std::path::Path::new("icons/icon.ico");
    if !ico.exists() {
        if let Some(parent) = ico.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        // minimal valid ICO wrapping a 32x32 BMP-like PNG is complex; write empty marker
        // and also emit png-based icon via png only. Create a tiny valid ICO (1 PNG image).
        if let Ok(png) = std::fs::read("icons/32x32.png") {
            if let Some(ico_bytes) = wrap_png_in_ico(&png, 32) {
                let _ = std::fs::write(ico, ico_bytes);
            }
        }
    }
    tauri_build::build()
}

fn write_png(path: &std::path::Path, size: u32, r: u8, g: u8, b: u8) {
    // minimal uncompressed-style PNG via image-less encoder
    let raw = png_bytes(size, r, g, b);
    let mut f = std::fs::File::create(path).expect("create icon");
    f.write_all(&raw).expect("write icon");
}

fn png_bytes(size: u32, r: u8, g: u8, b: u8) -> Vec<u8> {
    // Use a tiny hand-rolled RGB PNG (filter 0 rows).
    fn crc32(data: &[u8]) -> u32 {
        let mut table = [0u32; 256];
        for i in 0..256u32 {
            let mut c = i;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xEDB88320 ^ (c >> 1) } else { c >> 1 };
            }
            table[i as usize] = c;
        }
        let mut c = 0xFFFF_FFFFu32;
        for byte in data {
            c = table[((c ^ *byte as u32) & 0xFF) as usize] ^ (c >> 8);
        }
        c ^ 0xFFFF_FFFF
    }
    fn chunk(tag: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        out.extend_from_slice(tag);
        out.extend_from_slice(data);
        let mut crc_input = tag.to_vec();
        crc_input.extend_from_slice(data);
        out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
        out
    }

    let mut ihdr = Vec::new();
    ihdr.extend_from_slice(&size.to_be_bytes());
    ihdr.extend_from_slice(&size.to_be_bytes());
    ihdr.extend_from_slice(&[8, 2, 0, 0, 0]); // 8-bit RGB

    let mut raw = Vec::new();
    for _y in 0..size {
        raw.push(0); // filter none
        for _x in 0..size {
            raw.extend_from_slice(&[r, g, b]);
        }
    }
    // store uncompressed deflate blocks
    let mut zlib = Vec::new();
    zlib.push(0x78);
    zlib.push(0x01);
    let mut i = 0;
    while i < raw.len() {
        let chunk_len = (raw.len() - i).min(65535);
        let last = if i + chunk_len >= raw.len() { 1u8 } else { 0u8 };
        zlib.push(last);
        zlib.extend_from_slice(&(chunk_len as u16).to_le_bytes());
        zlib.extend_from_slice(&(!(chunk_len as u16)).to_le_bytes());
        zlib.extend_from_slice(&raw[i..i + chunk_len]);
        i += chunk_len;
    }
    // adler32
    let mut a = 1u32;
    let mut bsum = 0u32;
    for byte in &raw {
        a = (a + *byte as u32) % 65521;
        bsum = (bsum + a) % 65521;
    }
    zlib.extend_from_slice(&(((bsum << 16) | a) as u32).to_be_bytes());

    let mut out = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    out.extend(chunk(b"IHDR", &ihdr));
    out.extend(chunk(b"IDAT", &zlib));
    out.extend(chunk(b"IEND", &[]));
    out
}

fn wrap_png_in_ico(png: &[u8], size: u32) -> Option<Vec<u8>> {
    let mut ico = Vec::new();
    ico.extend_from_slice(&0u16.to_le_bytes());
    ico.extend_from_slice(&1u16.to_le_bytes()); // 1 image
    ico.push(size.min(255) as u8);
    ico.push(size.min(255) as u8);
    ico.push(0);
    ico.push(0);
    ico.extend_from_slice(&1u16.to_le_bytes());
    ico.extend_from_slice(&32u16.to_le_bytes());
    ico.extend_from_slice(&(png.len() as u32).to_le_bytes());
    ico.extend_from_slice(&22u32.to_le_bytes());
    ico.extend_from_slice(png);
    Some(ico)
}

#[allow(dead_code)]
fn unused_command() {
    let _ = Command::new("echo");
}
