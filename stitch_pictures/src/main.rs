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

fn build_tile_img(tile: &Tile) -> bool {
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
                None => {
                    println!("{tile:?} cannot render: {t:?}: no tile");
                    return false;
                }
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
                None => {
                    println!("{tile:?} cannot render: {t:?}: no outline");
                    return false;
                }
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

    true
}

fn main() {
    let mut tiles_touched = HashSet::new();

    let mut files = std::fs::read_dir("../tiles")
        .unwrap()
        .map(|v| v.unwrap().file_name().to_string_lossy().to_string())
        .collect::<Vec<_>>();
    files.sort();

    let mut all_tiles = vec![];

    for name in files.into_iter() {
        // let img = image::io::Reader::open(format!("tiles/{name}"))
        //     .unwrap()
        //     .decode()
        //     .unwrap();
        let mut parts = name.strip_suffix(".jpg").unwrap().split("-");
        let y = parts.next().unwrap().parse().unwrap();
        let x = parts.next().unwrap().parse().unwrap();
        let tile = Tile::new(ZOOM, x, y).unwrap();
        all_tiles.push(tile);
    }

    println!("{}", all_tiles.len());

    let mut ok = 0;
    let mut fail = 0;

    all_tiles.sort_by_key(|v| (v.x(), v.y()));
    for tile in all_tiles.into_iter().progress_with_style(ProgressStyle::with_template(
        "[{elapsed_precise}->{eta_precise}] {bar:100} [{human_pos}/{human_len} {percent}% {per_sec}]",
    )
    .unwrap())
    {
        if tile.x() % 8 != 0 {continue;}
        if tile.y() % 8 != 0 {continue;}
        if !tiles_touched.contains(&tile) {
            if build_tile_img(&tile) {
                ok += 1;
            } else {
                fail += 1;
            }
            for dx in 0..8 {
                for dy in 0..8 {
                    tiles_touched.insert(Tile::new(ZOOM, tile.x() + dx, tile.y() + dy).unwrap());
                }
            }
        }
    }
    println!("OK: {ok}, error: {fail}");
}
