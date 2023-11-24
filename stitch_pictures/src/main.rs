use std::collections::HashSet;

use image::{DynamicImage, RgbImage};
use indicatif::{ProgressIterator, ProgressStyle};
use slippy_map_tiles::Tile;

const ZOOM: u8 = 17; // zoom where 1px=1m;

fn get_tile(t: Tile) -> Option<DynamicImage> {
    image::io::Reader::open(format!("../tiles/{}-{}.jpg", t.y(), t.x()))
        .ok()?
        .decode()
        .ok()
}

fn get_outline(t: Tile) -> Option<DynamicImage> {
    image::io::Reader::open(format!("../outlines/{}-{}.png", t.y(), t.x()))
        .ok()?
        .decode()
        .ok()
}

fn build_tile_img(tile: &Tile) {
    let mut target_tile = RgbImage::new(256 * 8, 256 * 8);
    let mut target_outline = RgbImage::new(256 * 8, 256 * 8);
    for x in 0..8 {
        for y in 0..8 {
            let t = Tile::new(ZOOM, tile.x() + x, tile.y() + y).unwrap();
            match get_tile(t) {
                Some(img) => {
                    image::imageops::overlay(
                        &mut target_tile,
                        &img.clone().into_rgb8(),
                        img.width() as i64 * x as i64,
                        img.height() as i64 * y as i64,
                    );
                }
                None => return,
            };

            match get_outline(t) {
                Some(img) => {
                    image::imageops::overlay(
                        &mut target_outline,
                        &img.clone().into_rgb8(),
                        img.width() as i64 * x as i64,
                        img.height() as i64 * y as i64,
                    );
                }
                None => return,
            };
        }
    }

    target_tile
        .save(format!("../stitched/tiles/{}-{}.jpg", tile.y(), tile.x()))
        .unwrap();
    target_outline
        .save(format!(
            "../stitched/outlines/{}-{}.png",
            tile.y(),
            tile.x()
        ))
        .unwrap();
}

fn main() {
    let mut tiles_touched = HashSet::new();

    let mut files = std::fs::read_dir("../tiles")
        .unwrap()
        .map(|v| v.unwrap().file_name().to_string_lossy().to_string())
        .collect::<Vec<_>>();
    files.sort();
    for name in files.into_iter()
        .progress_with_style(ProgressStyle::with_template(
            "[{elapsed_precise}->{eta_precise}] {bar:100} [{human_pos}/{human_len} {percent}% {per_sec}]",
        )
        .unwrap())
    {
        // let img = image::io::Reader::open(format!("tiles/{name}"))
        //     .unwrap()
        //     .decode()
        //     .unwrap();
        let mut parts = name.strip_suffix(".jpg").unwrap().split("-");
        let y = parts.next().unwrap().parse().unwrap();
        let x = parts.next().unwrap().parse().unwrap();
        let tile = Tile::new(ZOOM, x, y).unwrap();
        if !tiles_touched.contains(&tile) {
            build_tile_img(&tile);
            for dx in 0..8 {
                for dy in 0..8 {
                    tiles_touched.insert(Tile::new(ZOOM, x + dx, y + dy).unwrap());
                }
            }
        }
    }
}
