#[cfg(windows)]
fn lerp_u8(a: u8, b: u8, t: f32) -> u8 {
    (a as f32 + (b as f32 - a as f32) * t.clamp(0.0, 1.0)).round() as u8
}

#[cfg(windows)]
fn pink_white_gradient(t: f32) -> [u8; 4] {
    // Diagonal Sakura Pink (#FF4D94) -> Soft Blush (#FFA6D2) -> Pearl White (#FFF5FA)
    let t = t.clamp(0.0, 1.0);
    if t < 0.55 {
        let k = t / 0.55;
        [lerp_u8(255, 255, k), lerp_u8(77, 166, k), lerp_u8(148, 210, k), 255]
    } else {
        let k = (t - 0.55) / 0.45;
        [lerp_u8(255, 255, k), lerp_u8(166, 245, k), lerp_u8(210, 250, k), 255]
    }
}

#[cfg(windows)]
fn render_icon_rgba(size: u32) -> Vec<u8> {
    let mut rgba = vec![0u8; (size * size * 4) as usize];
    let border = 2.0 / 32.0;
    for y in 0..size {
        for x in 0..size {
            let idx = ((y * size + x) * 4) as usize;
            let u = (x as f32 + 0.5) / size as f32;
            let v = (y as f32 + 0.5) / size as f32;

            let in_border = u < border || u > 1.0 - border || v < border || v > 1.0 - border;
            let du = (u - 0.5).abs();
            let arm_x = (0.78 - v) * 0.46;
            let in_v = (0.24..=0.78).contains(&v) && (du - arm_x).abs() <= 0.078;
            let dot_dist = ((u - 0.5).powi(2) + (v - 0.28).powi(2)).sqrt();
            let in_dot = dot_dist <= 0.075;

            let px: [u8; 4] = if in_border || in_v || in_dot {
                let grad_t = if in_dot { 0.92 } else { u * 0.55 + (1.0 - v) * 0.45 };
                pink_white_gradient(grad_t)
            } else {
                // Pure AMOLED Black with subtle midnight depth (#000000 -> #0A080E)
                [lerp_u8(0, 10, v), lerp_u8(0, 8, v), lerp_u8(0, 14, v), 255]
            };
            rgba[idx..idx + 4].copy_from_slice(&px);
        }
    }
    rgba
}

#[cfg(windows)]
fn build_dib_entry(size: u32) -> Vec<u8> {
    let rgba = render_icon_rgba(size);
    let xor_len = size * size * 4;
    let and_row_bytes = ((size + 31) / 32) * 4;
    let and_len = and_row_bytes * size;

    let mut dib = Vec::with_capacity((40 + xor_len + and_len) as usize);
    // BITMAPINFOHEADER (40 bytes)
    dib.extend_from_slice(&40u32.to_le_bytes());
    dib.extend_from_slice(&(size as i32).to_le_bytes());
    dib.extend_from_slice(&((size * 2) as i32).to_le_bytes()); // height * 2 (XOR + AND)
    dib.extend_from_slice(&1u16.to_le_bytes()); // planes
    dib.extend_from_slice(&32u16.to_le_bytes()); // 32-bit BGRA
    dib.extend_from_slice(&0u32.to_le_bytes()); // BI_RGB
    dib.extend_from_slice(&(xor_len + and_len).to_le_bytes());
    dib.extend_from_slice(&[0u8; 16]); // ppm + color counts

    // Bottom-up BGRA pixels
    for y in (0..size).rev() {
        for x in 0..size {
            let idx = ((y * size + x) * 4) as usize;
            dib.push(rgba[idx + 2]); // B
            dib.push(rgba[idx + 1]); // G
            dib.push(rgba[idx]); // R
            dib.push(rgba[idx + 3]); // A
        }
    }

    // 1bpp AND mask (all 0 = opaque, alpha channel controls transparency)
    dib.resize(dib.len() + and_len as usize, 0u8);
    dib
}

#[cfg(windows)]
fn build_multi_res_ico(sizes: &[u32]) -> Vec<u8> {
    let entries: Vec<(u32, Vec<u8>)> = sizes.iter().map(|&s| (s, build_dib_entry(s))).collect();
    let mut ico = Vec::new();

    // ICONDIR (6 bytes)
    ico.extend_from_slice(&0u16.to_le_bytes()); // reserved
    ico.extend_from_slice(&1u16.to_le_bytes()); // type = ICO
    ico.extend_from_slice(&(entries.len() as u16).to_le_bytes());

    // ICONDIRENTRY table (16 bytes each)
    let mut offset = 6u32 + (entries.len() as u32) * 16;
    for (size, dib) in &entries {
        ico.push(*size as u8);
        ico.push(*size as u8);
        ico.push(0); // color count
        ico.push(0); // reserved
        ico.extend_from_slice(&1u16.to_le_bytes()); // planes
        ico.extend_from_slice(&32u16.to_le_bytes()); // bit count
        ico.extend_from_slice(&(dib.len() as u32).to_le_bytes());
        ico.extend_from_slice(&offset.to_le_bytes());
        offset += dib.len() as u32;
    }

    for (_, dib) in entries {
        ico.extend_from_slice(&dib);
    }
    ico
}

fn emit_build_metadata() {
    println!("cargo:rerun-if-env-changed=GITHUB_SHA");
    let sha = std::env::var("GITHUB_SHA")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| {
            std::process::Command::new("git")
                .args(["rev-parse", "--short=7", "HEAD"])
                .output()
                .ok()
                .filter(|o| o.status.success())
                .and_then(|o| String::from_utf8(o.stdout).ok())
        })
        .map(|s| s.trim().chars().take(7).collect::<String>())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "local".to_string());
    println!("cargo:rustc-env=APP_BUILD_SHA={sha}");
}

#[cfg(windows)]
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    emit_build_metadata();
    let out_dir = std::path::PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR not set"));
    let ico_path = out_dir.join("app.ico");
    std::fs::write(&ico_path, build_multi_res_ico(&[16, 32, 48, 64]))
        .expect("failed to write generated app.ico");

    // Forward slashes prevent Windows rc.exe from treating `\t` in `\target` as an escape sequence.
    let ico_str = ico_path.to_string_lossy().replace('\\', "/");
    let mut res = winres::WindowsResource::new();
    res.set_icon(&ico_str);
    res.set("ProductName", "Valorant Streamer Radar");
    res.set("FileDescription", "Valorant Streamer Radar");
    res.compile().expect("failed to compile Windows icon resource");
}

#[cfg(not(windows))]
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    emit_build_metadata();
}
