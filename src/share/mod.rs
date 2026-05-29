use std::io::Write;

use anyhow::{Result, anyhow};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::{DateTime, Utc};
use flate2::Compression;
use flate2::write::{DeflateDecoder, DeflateEncoder};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const PAYLOAD_PREFIX: &str = "czoo1:";

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "payload_kind")]
pub enum Payload {
    #[serde(rename = "gift")]
    Gift(GiftPayload),
    #[serde(rename = "snapshot")]
    Snapshot(SharedSnapshotPayload),
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct GiftPayload {
    pub version: u32,
    pub gift_id: Uuid,
    pub sender_id: Uuid,
    pub sender_name: String,
    pub created_at: DateTime<Utc>,
    pub contents: GiftContents,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "kind")]
pub enum GiftContents {
    #[serde(rename = "animal")]
    Animal { species_id: String, level: u8 },
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SharedSnapshotPayload {
    pub version: u32,
    pub sender_id: Uuid,
    pub sender_name: String,
    pub taken_at: DateTime<Utc>,
    pub view: SnapshotView,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SnapshotView {
    pub coins: u64,
    pub food: u64,
    pub habitat_count: usize,
    pub structure_count: usize,
    pub animal_count: usize,
    /// (species_id, count, total_level) summarised so QR payloads stay compact.
    pub species_tally: Vec<SpeciesTallyEntry>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SpeciesTallyEntry {
    pub species_id: String,
    pub count: usize,
    pub total_level: u32,
}

/// Encode a payload as a multi-line share code:
///
/// ```text
/// cmd_zoo gift from Alex: Field Mouse (L3)
/// czoo1:<base64-deflate-json>
/// ```
///
/// The leading human line makes the QR informative to a phone scanner; the
/// `czoo1:` line is the machine-readable part. `decode` scans either line.
pub fn encode(payload: &Payload) -> Result<String> {
    let json = serde_json::to_vec(payload)?;
    let mut deflater = DeflateEncoder::new(Vec::new(), Compression::best());
    deflater.write_all(&json)?;
    let compressed = deflater.finish()?;
    let b64 = URL_SAFE_NO_PAD.encode(compressed);
    let machine = format!("{PAYLOAD_PREFIX}{b64}");
    let preface = human_preface(payload);
    Ok(format!("{preface}\n{machine}"))
}

/// Produce a single-line, ASCII-friendly summary of a payload. Species ids
/// are resolved through `crate::game::species` for nicer display names; if
/// resolution fails (unknown id from a future build), the raw id is shown.
fn human_preface(payload: &Payload) -> String {
    match payload {
        Payload::Gift(g) => {
            let GiftContents::Animal { species_id, level } = &g.contents;
            let name = crate::game::species::try_get(species_id)
                .map(|d| d.display_name.to_string())
                .unwrap_or_else(|| species_id.clone());
            format!(
                "cmd_zoo gift from {sender}: {name} (L{level})",
                sender = g.sender_name
            )
        }
        Payload::Snapshot(s) => format!(
            "cmd_zoo snapshot from {sender}: {animals} animals, {habitats} habitats",
            sender = s.sender_name,
            animals = s.view.animal_count,
            habitats = s.view.habitat_count,
        ),
    }
}

/// Render a share code as a Unicode-half-block QR. Returns an error if the
/// code is too long to fit even the largest QR version.
pub fn render_qr_ascii(code: &str) -> Result<String> {
    let qr = qrcode::QrCode::new(code.as_bytes())
        .map_err(|e| anyhow!("code is too long for a QR ({e})"))?;
    Ok(qr
        .render::<qrcode::render::unicode::Dense1x2>()
        .quiet_zone(false)
        .build())
}

/// Render a share code as a square `(width, rgba_pixels)` bitmap. Each QR
/// module becomes `module_px × module_px` true-pixels; a `quiet_zone_modules`
/// border of light pixels is included so scanners lock on cleanly.
///
/// Caller is expected to upload this as a texture (e.g. via
/// `egui::ColorImage::from_rgba_unmultiplied`).
pub fn render_qr_rgba(
    code: &str,
    module_px: usize,
    quiet_zone_modules: usize,
) -> Result<(usize, Vec<u8>)> {
    let qr = qrcode::QrCode::new(code.as_bytes())
        .map_err(|e| anyhow!("code is too long for a QR ({e})"))?;
    let colors: Vec<qrcode::Color> = qr.to_colors();
    let modules_side = (colors.len() as f64).sqrt() as usize;
    debug_assert_eq!(modules_side * modules_side, colors.len());

    let total_modules = modules_side + 2 * quiet_zone_modules;
    let side_px = total_modules * module_px;
    let mut rgba = vec![0u8; side_px * side_px * 4];

    let dark: [u8; 4] = [0x0e, 0x0f, 0x12, 0xff];
    let light: [u8; 4] = [0xe6, 0xe8, 0xeb, 0xff];

    for ty in 0..total_modules {
        for tx in 0..total_modules {
            let in_qr = ty >= quiet_zone_modules
                && tx >= quiet_zone_modules
                && ty < quiet_zone_modules + modules_side
                && tx < quiet_zone_modules + modules_side;
            let is_dark = in_qr
                && matches!(
                    colors[(ty - quiet_zone_modules) * modules_side + (tx - quiet_zone_modules)],
                    qrcode::Color::Dark
                );
            let color = if is_dark { dark } else { light };
            for py in 0..module_px {
                for px in 0..module_px {
                    let y = ty * module_px + py;
                    let x = tx * module_px + px;
                    let i = (y * side_px + x) * 4;
                    rgba[i] = color[0];
                    rgba[i + 1] = color[1];
                    rgba[i + 2] = color[2];
                    rgba[i + 3] = color[3];
                }
            }
        }
    }

    Ok((side_px, rgba))
}

/// Parse a share code. Tolerant of preceding/trailing junk lines (e.g. a
/// human-readable prefix line, or trailing whitespace from a paste); finds
/// the first line that begins with `czoo1:` and parses that.
pub fn decode(input: &str) -> Result<Payload> {
    let line = input
        .lines()
        .find_map(|l| l.trim().strip_prefix(PAYLOAD_PREFIX))
        .ok_or_else(|| anyhow!("no {PAYLOAD_PREFIX:?} line found in input"))?;
    let compressed = URL_SAFE_NO_PAD
        .decode(line.as_bytes())
        .map_err(|e| anyhow!("base64 decode: {e}"))?;
    let mut inflater = DeflateDecoder::new(Vec::new());
    inflater.write_all(&compressed)?;
    let json = inflater.finish()?;
    let payload: Payload = serde_json::from_slice(&json)?;
    Ok(payload)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn ts() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 5, 28, 9, 0, 0).unwrap()
    }

    #[test]
    fn gift_roundtrip_identity() {
        let p = Payload::Gift(GiftPayload {
            version: 1,
            gift_id: Uuid::new_v4(),
            sender_id: Uuid::new_v4(),
            sender_name: "Alex".into(),
            created_at: ts(),
            contents: GiftContents::Animal {
                species_id: "fox".into(),
                level: 3,
            },
        });
        let code = encode(&p).unwrap();
        // Encoded form is multi-line: a human preface, then the czoo line.
        let mut lines = code.lines();
        let first = lines.next().unwrap();
        assert!(
            first.starts_with("cmd_zoo gift from Alex:"),
            "preface line missing: {first}"
        );
        assert!(first.contains("Red Fox"), "species name missing: {first}");
        assert!(first.contains("(L3)"));
        let second = lines.next().unwrap();
        assert!(second.starts_with(PAYLOAD_PREFIX));
        let decoded = decode(&code).unwrap();
        match (p, decoded) {
            (Payload::Gift(a), Payload::Gift(b)) => {
                assert_eq!(a.gift_id, b.gift_id);
                assert_eq!(a.sender_id, b.sender_id);
                assert_eq!(a.sender_name, b.sender_name);
                assert_eq!(a.created_at, b.created_at);
                match (a.contents, b.contents) {
                    (
                        GiftContents::Animal { species_id, level },
                        GiftContents::Animal {
                            species_id: s2,
                            level: l2,
                        },
                    ) => {
                        assert_eq!(species_id, s2);
                        assert_eq!(level, l2);
                    }
                }
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn snapshot_roundtrip_identity() {
        let p = Payload::Snapshot(SharedSnapshotPayload {
            version: 1,
            sender_id: Uuid::new_v4(),
            sender_name: "Alex".into(),
            taken_at: ts(),
            view: SnapshotView {
                coins: 5000,
                food: 200,
                habitat_count: 3,
                structure_count: 2,
                animal_count: 7,
                species_tally: vec![SpeciesTallyEntry {
                    species_id: "fieldMouse".into(),
                    count: 4,
                    total_level: 5,
                }],
            },
        });
        let code = encode(&p).unwrap();
        let decoded = decode(&code).unwrap();
        match decoded {
            Payload::Snapshot(s) => {
                assert_eq!(s.view.coins, 5000);
                assert_eq!(s.view.species_tally.len(), 1);
                assert_eq!(s.view.species_tally[0].species_id, "fieldMouse");
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn qr_rgba_is_square_with_expected_pixel_count() {
        // 4-pixel module + 4-module quiet zone on each side.
        let (side, bytes) = super::render_qr_rgba("czoo1:abc", 4, 4).unwrap();
        assert!(side >= 4 * (4 + 4 + 4)); // at least quiet+min-qr+quiet modules wide
        assert_eq!(bytes.len(), side * side * 4);
        // Verify at least one dark and one light pixel landed in the buffer.
        let dark = bytes.chunks(4).any(|p| p[0] < 0x40);
        let light = bytes.chunks(4).any(|p| p[0] > 0xc0);
        assert!(dark, "expected at least one dark pixel");
        assert!(light, "expected at least one light pixel");
    }

    #[test]
    fn decode_rejects_missing_prefix() {
        assert!(decode("not-a-share-code").is_err());
    }

    /// Lone czoo line (no human preface) still decodes — covers legacy and
    /// minimalist paste paths.
    #[test]
    fn decode_finds_czoo_line_alone() {
        let p = Payload::Gift(GiftPayload {
            version: 1,
            gift_id: Uuid::new_v4(),
            sender_id: Uuid::new_v4(),
            sender_name: "Alex".into(),
            created_at: ts(),
            contents: GiftContents::Animal {
                species_id: "fieldMouse".into(),
                level: 1,
            },
        });
        let full = encode(&p).unwrap();
        // Strip the preface, keep only the czoo: line.
        let machine_only = full.lines().find(|l| l.starts_with(PAYLOAD_PREFIX)).unwrap();
        assert!(decode(machine_only).is_ok());
    }

    /// Scanner output frequently has stray whitespace / OCR garbage. As long as
    /// some line begins with the prefix, we still parse it.
    #[test]
    fn decode_finds_czoo_line_amid_garbage() {
        let p = Payload::Snapshot(SharedSnapshotPayload {
            version: 1,
            sender_id: Uuid::new_v4(),
            sender_name: "Alex".into(),
            taken_at: ts(),
            view: SnapshotView {
                coins: 1,
                food: 0,
                habitat_count: 1,
                structure_count: 0,
                animal_count: 0,
                species_tally: Vec::new(),
            },
        });
        let code = encode(&p).unwrap();
        let machine = code.lines().find(|l| l.starts_with(PAYLOAD_PREFIX)).unwrap();
        let messy = format!("random preface line\n\n{machine}\n\ntrailing garbage");
        assert!(decode(&messy).is_ok());
    }
}
