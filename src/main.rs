use std::{
    collections::{HashMap, HashSet},
    ffi::OsString,
    io::{BufReader, BufWriter, Cursor},
    str::FromStr,
};

use geo::{Coord, GeodesicArea, LineString, Polygon};
use image::{DynamicImage, ImageBuffer};
use imageproc::point::Point;
use indicatif::{ProgressBar, ProgressIterator, ProgressStyle};
use log::{debug, info, warn};
use osmpbfreader::{Node, Relation, Way};
use rayon::iter::{IntoParallelIterator, ParallelIterator};
use slippy_map_tiles::{lat_lon_to_tile, zorder_to_xy, BBox, LatLon, Tile};

struct ProgressFile<R: std::io::Read> {
    inner: R,
    progress: indicatif::ProgressBar,
}

impl<R: std::io::Read> ProgressFile<R> {
    pub fn new(inner: R, len: u64) -> Self {
        Self {
            inner,
            progress: ProgressBar::new(len).with_style(
                ProgressStyle::with_template(
                    "[{eta_precise}] {bar:120} [{bytes}/{total_bytes} {percent}%]",
                )
                .unwrap(),
            ),
        }
    }
}

impl<R: std::io::Read> std::io::Read for ProgressFile<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let count = self.inner.read(buf)?;
        self.progress.inc(count as u64);
        Ok(count)
    }
}

fn fetch_buildings(filename: &std::ffi::OsStr) {
    let r = std::fs::File::open(&std::path::Path::new(filename)).unwrap();
    let len = r.metadata().unwrap().len();
    let r = ProgressFile::new(r, len);
    let mut pbf = osmpbfreader::OsmPbfReader::new(r);

    let mut nodes_all = HashMap::new();
    let mut nodes_only_buildings = HashMap::new();
    let mut ways_buildings = HashMap::new();
    let mut relations_buildings = HashMap::new();

    for obj in pbf.par_iter().map(Result::unwrap) {
        let is_building = obj.tags().contains_key("building");
        match obj {
            osmpbfreader::OsmObj::Node(node) => {
                if is_building {
                    nodes_only_buildings.insert(node.id.0, node.clone());
                }
                nodes_all.insert(node.id.0, node);
            }
            osmpbfreader::OsmObj::Way(way) => {
                if is_building {
                    ways_buildings.insert(way.id.0, way);
                }
            }
            osmpbfreader::OsmObj::Relation(rel) => {
                if is_building {
                    relations_buildings.insert(rel.id.0, rel);
                }
            }
        }
    }

    println!("All nodes: {}", nodes_all.len());
    println!("Building nodes: {}", nodes_only_buildings.len());
    println!("Building ways: {}", ways_buildings.len());
    println!("Building relations: {}", relations_buildings.len());
}

const ZOOM: u8 = 17; // zoom where 1px=1m;

fn translate(value: f64, left_min: f64, left_max: f64, right_min: f64, right_max: f64) -> f64 {
    log::trace!("translate({value}, {left_min}, {left_max}, {right_min}, {right_max}");
    let left_span = left_max - left_min;
    let right_span = right_max - right_min;

    let value_scaled = (value - left_min) / left_span;

    let out = right_min + (value_scaled * right_span);

    log::trace!("translate({value}, {left_min}, {left_max}, {right_min}, {right_max}) -> {out}");
    out
}

#[derive(Clone, Copy, Debug)]
struct GeoCoordinate {
    pub longitude: f64,
    pub latitude: f64,
}

impl From<Point<f64>> for GeoCoordinate {
    fn from(value: Point<f64>) -> Self {
        Self {
            longitude: value.x,
            latitude: value.y,
        }
    }
}

impl From<Coord<f64>> for GeoCoordinate {
    fn from(value: Coord<f64>) -> Self {
        Self {
            longitude: value.x,
            latitude: value.y,
        }
    }
}

impl From<GeoCoordinate> for Coord<f64> {
    fn from(value: GeoCoordinate) -> Self {
        Coord {
            x: value.longitude,
            y: value.latitude,
        }
    }
}

const COLOR_INDEX: &[[u8; 3]] = &[[0, 0, 0], [255, 0, 0], [0, 255, 0]];

#[derive(Clone, Copy, Debug)]
enum BuildingColor {
    Nothing = 0,
    BuildingBelowAreaThreshold = 1,
    Normal = 2,
    BuildingHasExcludedTags = 3,
}

#[derive(Default)]
struct ImageCache {
    tiles: HashMap<Tile, ()>,
    outlines: HashMap<Tile, ImageBuffer<image::Rgb<u8>, Vec<u8>>>,
    dirty: HashSet<Tile>,
    client: reqwest::blocking::Client,
}

impl ImageCache {
    pub fn prepare_tile(&mut self, tile: Tile) -> anyhow::Result<()> {
        // let interest_center = (54.6961, 20.5120);
        // let interest_zoom = ZOOM - 4; // 4096 area

        // let interest_megatile =
        //     slippy_map_tiles::lat_lon_to_tile(interest_center.0, interest_center.1, interest_zoom);
        // let interest_megatile =
        //     Tile::new(interest_zoom, interest_megatile.0, interest_megatile.1).unwrap();

        // entire moscow
        let buf = 0.5;
        let interest_bbox =
            slippy_map_tiles::BBox::new(55.93 + buf, 37.3 - buf, 55.56 - buf, 37.9 + buf).unwrap();

        // inside TTK
        // let interest_bbox = slippy_map_tiles::BBox::new(55.79, 37.53, 55.70, 37.7).unwrap();

        let do_download = {
            interest_bbox.overlaps_bbox(&tile.bbox())
            // let mut t = tile;
            // while t.zoom() > interest_megatile.zoom() {
            //     t = t.parent().unwrap();
            // }
            // t == interest_megatile
        };

        if self.tiles.get(&tile).is_none() && !do_download {
            anyhow::bail!("Missing tile, and not downloading it");
        }

        if let (Some(_a), Some(_b)) = (self.tiles.get_mut(&tile), self.outlines.get_mut(&tile)) {
            return Ok(());
        }

        info!("Preparing tile {tile:?}");

        assert_eq!(tile.zoom(), ZOOM);

        let path = format!("https://server.arcgisonline.com/ArcGIS/rest/services/World_Imagery/MapServer/tile/{}/{}/{}", tile.zoom(), tile.y(), tile.x());
        //let path = format!("https://core-sat.maps.yandex.net/tiles?l=sat&v=3.1124.0&x={}&y={}&z={}&scale=1&lang=ru_RU&client_id=yandex-web-maps", tile.x(), tile.y(), tile.zoom());

        let tiledata = self
            .client
            .get(path)
            .send()
            .unwrap()
            .bytes()
            .unwrap()
            .to_vec();
        let tileimg = image::io::Reader::new(Cursor::new(tiledata))
            .with_guessed_format()
            .unwrap()
            .decode()
            .unwrap();
        let outline_img: ImageBuffer<image::Rgb<u8>, Vec<_>> =
            ImageBuffer::new(tileimg.width(), tileimg.height());

        tileimg
            .save(format!("tiles/{}-{}.jpg", tile.y(), tile.x()))
            .unwrap();
        self.tiles.insert(tile, ());
        self.outlines.insert(tile, outline_img);

        Ok(())
    }

    pub fn geo_to_screen_coordinate(
        tile: Tile,
        screen_size: (u32, u32),
        coord: GeoCoordinate,
    ) -> Point<i32> {
        let top_lat = tile.top() as f64;
        let bot_lat = tile.bottom() as f64;
        let left_lon = tile.left() as f64;
        let right_lon = tile.right() as f64;

        let x = translate(
            coord.longitude,
            left_lon,
            right_lon,
            0.0,
            screen_size.0 as f64,
        ) as i32;
        let y = translate(coord.latitude, top_lat, bot_lat, 0.0, screen_size.1 as f64) as i32;
        // NOTE: latitude is vertical coordinate, +Y is down
        // longitude is horizontal coordinate, and +X is right

        // println!("{lon_px} {lat_px}");
        Point::new(x, y)
    }

    pub fn draw_polygon(
        &mut self,
        poly: &[GeoCoordinate],
        how: BuildingColor,
    ) -> anyhow::Result<()> {
        info!("Drawing polygon {poly:?}");

        let tiles: HashSet<_> = poly
            .iter()
            .map(|v| {
                let c =
                    slippy_map_tiles::lat_lon_to_tile(v.latitude as f32, v.longitude as f32, ZOOM);
                Tile::new(ZOOM, c.0, c.1).unwrap()
            })
            .collect();

        for tile in tiles {
            debug!("Polygon is included in: {tile:?}");
            self.dirty.insert(tile);
            self.prepare_tile(tile)?;
            let img = self.outlines.get_mut(&tile).unwrap();
            let screen_size = (img.width(), img.height());

            let mut tile_relative_poly: Vec<_> = poly
                .iter()
                .map(|c| Self::geo_to_screen_coordinate(tile, screen_size, *c))
                .collect();
            while tile_relative_poly.last().unwrap() == tile_relative_poly.first().unwrap() {
                tile_relative_poly.pop().unwrap();
            }
            imageproc::drawing::draw_polygon_mut(
                img,
                &tile_relative_poly,
                image::Rgb(COLOR_INDEX[how as usize]),
            );
        }

        Ok(())
    }

    pub fn save(&mut self) {
        warn!("Saving image cache...");
        // for (tile, img) in self.tiles.iter().filter(|v| self.dirty.contains(v.0)) {
        //     img.save(format!("tiles/{}-{}.jpg", tile.y(), tile.x()))
        //         .unwrap();
        // }
        for (tile, img) in self.outlines.iter().filter(|v| self.dirty.contains(v.0)) {
            img.save(format!("outlines/{}-{}.png", tile.y(), tile.x()))
                .unwrap();
        }
        self.dirty.clear();
    }

    pub fn load() -> Self {
        warn!("Loading image cache...");
        let mut cache = Self::default();

        for name in std::fs::read_dir("tiles").unwrap() {
            let name = name.unwrap();
            let name = name.file_name();
            let name = name.to_string_lossy();
            // let img = image::io::Reader::open(format!("tiles/{name}"))
            //     .unwrap()
            //     .decode()
            //     .unwrap();
            let mut parts = name.strip_suffix(".jpg").unwrap().split("-");
            let y = parts.next().unwrap().parse().unwrap();
            let x = parts.next().unwrap().parse().unwrap();
            let tile = Tile::new(ZOOM, x, y).unwrap();
            cache.tiles.insert(tile, ());
        }
        for name in std::fs::read_dir("outlines").unwrap().collect::<Vec<_>>().into_iter().progress_with_style(
                ProgressStyle::with_template(
                    "[{elapsed_precise}->{eta_precise}] {bar:100} [{human_pos}/{human_len} {percent}% {per_sec}]",
                )
                .unwrap(),
            ) {
            let name = name.unwrap();
            let name = name.file_name();
            let name = name.to_string_lossy();
            let img = image::io::Reader::open(format!("outlines/{name}"))
                .unwrap()
                .decode()
                .unwrap();
            let mut parts = name.strip_suffix(".png").unwrap().split("-");
            let y = parts.next().unwrap().parse().unwrap();
            let x = parts.next().unwrap().parse().unwrap();
            let tile = Tile::new(ZOOM, x, y).unwrap();
            cache.outlines.insert(tile, img.into_rgb8());
        }

        info!(
            "Loaded {} tiles and {} outlines",
            cache.tiles.len(),
            cache.outlines.len()
        );

        cache
    }
}

fn fetch_outline_way(
    cache: &mut ImageCache,
    way: &Way,
    nodes: &HashMap<i64, Node>,
) -> anyhow::Result<()> {
    if way.nodes.len() < 3 {
        info!("This way has less than 3 nodes, ignoring");
        return Ok(());
    }
    let nodes: Vec<_> = way.nodes.iter().map(|v| nodes.get(&v.0)).collect();
    if !nodes.iter().all(|v| v.is_some()) {
        warn!("This way does not have all nodes available");
        return Ok(());
    }
    let coords: Vec<_> = nodes
        .iter()
        .map(|v| v.unwrap())
        .map(|n| GeoCoordinate {
            longitude: (n.decimicro_lon as f64) / 10_000_000.0,
            latitude: (n.decimicro_lat as f64) / 10_000_000.0,
        })
        .collect();

    let geo_poly = Polygon::new(
        LineString::new(coords.iter().map(|v| (*v).into()).collect()),
        vec![],
    );
    let area = geo_poly.geodesic_area_signed().abs();
    info!("Area: {area} m^2");
    if area < 100.0 {
        cache.draw_polygon(&coords, BuildingColor::BuildingBelowAreaThreshold)?;
    } else {
        cache.draw_polygon(&coords, BuildingColor::Normal)?;
    }
    Ok(())
}

fn build_outlines(filename: &std::ffi::OsStr) {
    println!("Loading...");
    // let r = std::fs::File::open(&std::path::Path::new(filename)).unwrap();
    // let len = r.metadata().unwrap().len();
    // let r = ProgressFile::new(r, len);
    // let mut pbf = osmpbfreader::OsmPbfReader::new(r);

    // let mut nodes_all = HashMap::new();
    // let mut nodes_only_buildings = HashMap::new();
    // let mut ways_all = HashMap::new();
    // let mut ways_buildings = HashMap::new();
    // let mut relations_buildings = HashMap::new();

    // for obj in pbf.par_iter().map(Result::unwrap) {
    //     let is_building = obj.tags().contains_key("building");
    //     match obj {
    //         osmpbfreader::OsmObj::Node(node) => {
    //             if is_building {
    //                 nodes_only_buildings.insert(node.id.0, node.clone());
    //             }
    //             nodes_all.insert(node.id.0, node);
    //         }
    //         osmpbfreader::OsmObj::Way(way) => {
    //             if is_building {
    //                 ways_buildings.insert(way.id.0, way.clone());
    //             }
    //             ways_all.insert(way.id.0, way);
    //         }
    //         osmpbfreader::OsmObj::Relation(rel) => {
    //             if is_building {
    //                 relations_buildings.insert(rel.id.0, rel);
    //             }
    //         }
    //     }
    // }
    println!("Loading imgs...");
    // let mut cache = ImageCache::load();
    println!("Done!");

    let client = reqwest::blocking::Client::new();

    let mut tiles = HashSet::new();
    for name in std::fs::read_dir("tiles").unwrap().collect::<Vec<_>>().into_iter().progress_with_style(
        ProgressStyle::with_template(
            "[{elapsed_precise}->{eta_precise}] {bar:100} [{human_pos}/{human_len} {percent}% {per_sec}]",
        )
        .unwrap(),
    ) {
    let name = name.unwrap();
    let name = name.file_name();
    let name = name.to_string_lossy();
    // let img = image::io::Reader::open(format!("outlines/{name}"))
    //     .unwrap()
    //     .decode()
    //     .unwrap();
    let mut parts = name.strip_suffix(".jpg").unwrap().split("-");
    let y = parts.next().unwrap().parse().unwrap();
    let x = parts.next().unwrap().parse().unwrap();
    let tile = Tile::new(ZOOM, x, y).unwrap();
    tiles.insert(tile);
}

    let download_tile = |tile: Tile| {
        if tiles.contains(&tile) {
            return;
        }
        let path = format!("https://server.arcgisonline.com/ArcGIS/rest/services/World_Imagery/MapServer/tile/{}/{}/{}", tile.zoom(), tile.y(), tile.x());
        //let path = format!("https://core-sat.maps.yandex.net/tiles?l=sat&v=3.1124.0&x={}&y={}&z={}&scale=1&lang=ru_RU&client_id=yandex-web-maps", tile.x(), tile.y(), tile.zoom());

        let tiledata = client.get(path).send().unwrap().bytes().unwrap().to_vec();
        let tileimg = image::io::Reader::new(Cursor::new(tiledata))
            .with_guessed_format()
            .unwrap()
            .decode()
            .unwrap();

        tileimg
            .save(format!("tiles/{}-{}.jpg", tile.y(), tile.x()))
            .unwrap();
    };

    let buf = 0.5;
    let interest_bbox =
        slippy_map_tiles::BBox::new(55.93 + buf, 37.3 - buf, 55.56 - buf, 37.9 + buf).unwrap();

    rayon::ThreadPoolBuilder::new()
        .num_threads(256)
        .build_global()
        .unwrap();
    let iter = {
        let top_left_tile = lat_lon_to_tile(interest_bbox.top(), interest_bbox.left(), ZOOM);
        let bottom_right_tile =
            lat_lon_to_tile(interest_bbox.bottom(), interest_bbox.right(), ZOOM);

        (top_left_tile.0..=bottom_right_tile.0)
            .into_par_iter()
            .flat_map(move |x| {
                (top_left_tile.1..=bottom_right_tile.1)
                    .into_par_iter()
                    .map(move |y| (x, y))
            })
            .map(move |(x, y)| Tile::new(ZOOM, x, y).unwrap())
    };

    let top_left_tile = lat_lon_to_tile(interest_bbox.top(), interest_bbox.left(), ZOOM);
    let bottom_right_tile = lat_lon_to_tile(interest_bbox.bottom(), interest_bbox.right(), ZOOM);

    let count = (bottom_right_tile.0 - top_left_tile.0) * (bottom_right_tile.1 - top_left_tile.1);

    let style = ProgressStyle::with_template(
        "[{elapsed_precise}->{eta_precise}] {bar:100} [{human_pos}/{human_len} {percent}% {per_sec}]",
    )
    .unwrap();

    let pb = ProgressBar::new(count as u64).with_style(style);

    iter.for_each(|v| {
        download_tile(v);
        pb.inc(1);
    });

    // let mut idx = 0;
    // for way in ways_buildings.iter().progress_with_style(
    //     ProgressStyle::with_template(
    //         "[{elapsed_precise}->{eta_precise}] {bar:100} [{human_pos}/{human_len} {percent}% {per_sec}]",
    //     )
    //     .unwrap(),
    // ) {
    //     let mut interest_tags = String::new();
    //     for tag in way.1.tags.iter() {
    //         if tag.0.starts_with("building") {
    //             let part = format!("{}={}; ", tag.0, tag.1);
    //             interest_tags.extend(part.chars());
    //         }
    //     }
    //     // info!("{way:?}");
    //     // println!("{interest_tags}");
    //     idx += 1;
    //     if let Err(why) = fetch_outline_way(&mut cache, way.1, &nodes_all) {
    //         info!("error fetching outline: {why}")
    //     };
    //     if idx % 100 == 0 {
    //         cache.save();
    //     }
    // }

    // cache.save();

    // for rel in relations_buildings.iter().take(50) {
    //     println!("------------");
    //     fetch_outline(rel.1, &nodes_all, &ways_all);
    // }
}

fn main() {
    env_logger::init();
    let file =
        &OsString::from_str("/home/danya/Downloads/central-fed-district-latest.osm.pbf").unwrap();
    //        &OsString::from_str("/home/danya/Downloads/kaliningrad-latest.osm.pbf").unwrap();
    // fetch_buildings(&file);
    build_outlines(&file);
}
