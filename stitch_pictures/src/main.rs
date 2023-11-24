use std::collections::HashMap;

use image::{DynamicImage, ImageBuffer, RgbImage};
use slippy_map_tiles::Tile;

#[derive(Default)]
struct ImageCache {
    tiles: HashMap<Tile, DynamicImage>,
    outlines: HashMap<Tile, ImageBuffer<image::Rgb<u8>, Vec<u8>>>,
}

const ZOOM: u8 = 17; // zoom where 1px=1m;

impl ImageCache {
    fn load() -> Self {
        let mut cache = ImageCache::default();
        for name in std::fs::read_dir("../tiles").unwrap() {
            let name = name.unwrap();
            let name = name.file_name();
            let name = name.to_string_lossy();
            let img = image::io::Reader::open(format!("../tiles/{name}"))
                .unwrap()
                .decode()
                .unwrap();
            let mut parts = name.strip_suffix(".jpg").unwrap().split("-");
            let y = parts.next().unwrap().parse().unwrap();
            let x = parts.next().unwrap().parse().unwrap();
            let tile = Tile::new(ZOOM, x, y).unwrap();
            cache.tiles.insert(tile, img);
        }
        for name in std::fs::read_dir("../outlines").unwrap() {
            let name = name.unwrap();
            let name = name.file_name();
            let name = name.to_string_lossy();
            let img = image::io::Reader::open(format!("../outlines/{name}"))
                .unwrap()
                .decode()
                .unwrap();
            let mut parts = name.strip_suffix(".png").unwrap().split("-");
            let y = parts.next().unwrap().parse().unwrap();
            let x = parts.next().unwrap().parse().unwrap();
            let tile = Tile::new(ZOOM, x, y).unwrap();
            cache.outlines.insert(tile, img.into_rgb8());
        }

        cache
    }
}

fn build_tile_img(cache: &ImageCache, tile: (&Tile, &DynamicImage)) {
    let mut target_tile = RgbImage::new(tile.1.width() * 4, tile.1.height() * 4);
    let mut target_outline = RgbImage::new(tile.1.width() * 4, tile.1.height() * 4);
    for x in 0..4 {
        for y in 0..4 {
            let t = Tile::new(ZOOM, tile.0.x() + x, tile.0.y() + y).unwrap();
            match cache.tiles.get(&t) {
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

            match cache.outlines.get(&t) {
                Some(img) => {
                    image::imageops::overlay(
                        &mut target_outline,
                        img,
                        img.width() as i64 * x as i64,
                        img.height() as i64 * y as i64,
                    );
                }
                None => return,
            };
        }
    }

    target_tile
        .save(format!(
            "../stitched/tiles/{}-{}.jpg",
            tile.0.y(),
            tile.0.x()
        ))
        .unwrap();
    target_outline
        .save(format!(
            "../stitched/outlines/{}-{}.png",
            tile.0.y(),
            tile.0.x()
        ))
        .unwrap();
}

fn main() {
    let cache = ImageCache::load();

    for tile in cache.tiles.iter() {
        println!("{:?}", tile.0);
        build_tile_img(&cache, tile);
    }
}
