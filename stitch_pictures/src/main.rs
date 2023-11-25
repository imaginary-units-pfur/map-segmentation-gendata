use std::{collections::HashSet, sync::mpsc, thread::spawn};

use image::{DynamicImage, RgbImage};
use indicatif::{ProgressBar, ProgressIterator, ProgressStyle};
use rayon::iter::{IntoParallelRefIterator, ParallelIterator};
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
    if std::fs::OpenOptions::new()
        .open(format!("../stitched/tiles/{}-{}.jpg", tile.y(), tile.x()))
        .is_ok()
    {
        return true; // already exists
    }

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
                    let mut error = RgbImage::new(256, 256);
                    error.chunks_exact_mut(3).for_each(|v| {
                        v[0] = 0;
                        v[1] = 0;
                        v[2] = 255;
                    });
                    image::imageops::overlay(
                        &mut target_outline,
                        &error,
                        error.width() as i64 * x as i64,
                        error.height() as i64 * y as i64,
                    );

                    // println!("{tile:?} cannot render: {t:?}: no outline");
                    // return false;
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

    let style = ProgressStyle::with_template(
        "[{elapsed_precise}->{eta_precise}] {bar:100} [{human_pos}/{human_len} {percent}% {per_sec}]",
    )
    .unwrap();

    let (tx, rx) = std::sync::mpsc::sync_channel(100);

    enum Request {
        Contains(Tile, mpsc::SyncSender<bool>),
        Add(Tile),
    }

    spawn(move || loop {
        let req: Request = rx.recv().unwrap();
        match req {
            Request::Contains(tile, tx) => tx.send(tiles_touched.contains(&tile)).unwrap(),
            Request::Add(tile) => {
                tiles_touched.insert(tile);
            }
        }
    });

    let pb = ProgressBar::new(all_tiles.len() as u64).with_style(style);
    all_tiles.sort_by_key(|v| (v.x(), v.y()));
    all_tiles.par_iter().for_each(|tile| {
        pb.inc(1);
        if tile.x() % 8 != 0 {
            return;
        }
        if tile.y() % 8 != 0 {
            return;
        }
        let (my_tx, my_rx) = mpsc::sync_channel(1);
        tx.send(Request::Contains(*tile, my_tx)).unwrap();

        if !my_rx.recv().unwrap() {
            if build_tile_img(&tile) {
            } else {
            }
            for dx in 0..8 {
                for dy in 0..8 {
                    tx.send(Request::Add(
                        Tile::new(ZOOM, tile.x() + dx, tile.y() + dy).unwrap(),
                    ))
                    .unwrap();
                }
            }
        }
    });
}
