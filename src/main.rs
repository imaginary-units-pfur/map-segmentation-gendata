use std::{
    collections::HashMap,
    ffi::OsString,
    io::{BufReader, BufWriter, Cursor},
    str::FromStr,
};

use geo::{Coord, GeodesicArea, LineString, Polygon};
use image::ImageBuffer;
use imageproc::point::Point;
use indicatif::{ProgressBar, ProgressStyle};
use osmpbfreader::{Node, Relation, Way};
use slippy_map_tiles::LatLon;

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
    let left_span = left_max - left_min;
    let right_span = right_max - right_min;

    let value_scaled = (value - left_min) / left_span;

    right_min + (value_scaled * right_span)
}

fn fetch_outline_way(way: &Way, nodes: &HashMap<i64, Node>) {
    if way.nodes.len() < 3 {
        println!("This way has less than 3 nodes, ignoring");
        return;
    }
    let nodes: Vec<_> = way.nodes.iter().map(|v| nodes.get(&v.0)).collect();
    if !nodes.iter().all(|v| v.is_some()) {
        println!("This way does not have all nodes available");
        return;
    }
    let coords: Vec<_> = nodes
        .iter()
        .map(|v| v.unwrap())
        .map(|n| {
            (
                (n.decimicro_lat as f64) / 10_000_000.0,
                (n.decimicro_lon as f64) / 10_000_000.0,
            )
        })
        .collect();

    let geo_poly = Polygon::new(
        LineString::new(
            coords
                .iter()
                .cloned()
                .map(|(x, y)| Coord { x, y })
                .collect(),
        ),
        vec![],
    );
    let area = geo_poly.geodesic_area_unsigned();
    println!("Area: {area} m^2");
    if area < 10.0 {
        println!("Area too small, ignoring");
        return;
    }

    let min_lat = coords
        .iter()
        .map(|v| v.0)
        .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
        .unwrap();
    let min_lon = coords
        .iter()
        .map(|v| v.1)
        .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
        .unwrap();
    let max_lat = coords
        .iter()
        .map(|v| v.0)
        .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
        .unwrap();
    let max_lon = coords
        .iter()
        .map(|v| v.1)
        .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
        .unwrap();

    let min = LatLon::new(min_lat as f32, min_lon as f32).unwrap();
    let max = LatLon::new(max_lat as f32, max_lon as f32).unwrap();
    if min.tile(ZOOM) != max.tile(ZOOM) {
        println!("Building does not fit in one tile");
    }
    let tile = min.tile(ZOOM);
    let path = format!("https://server.arcgisonline.com/ArcGIS/rest/services/World_Imagery/MapServer/tile/{}/{}/{}", tile.zoom(), tile.y(), tile.x());
    //let path = format!("https://core-sat.maps.yandex.net/tiles?l=sat&v=3.1124.0&x={}&y={}&z={}&scale=1&lang=ru_RU&client_id=yandex-web-maps", tile.x(), tile.y(), tile.zoom());
    println!("Building in tile: {path}");

    let mut poly = vec![];
    for coord in coords.iter() {
        println!("{coord:?}");
        let min_lat = tile.top() as f64;
        let max_lat = tile.bottom() as f64;
        let (min_lat, max_lat) = (min_lat.min(max_lat), max_lat.max(min_lat));
        let min_lon = tile.left() as f64;
        let max_lon = tile.right() as f64;
        let (min_lon, max_lon) = (min_lon.min(max_lon), max_lon.max(min_lon));

        let lon_px = translate(coord.0, min_lat, max_lat, 0.0, 255.0) as i32;
        let lat_px = translate(coord.1, min_lon, max_lon, 0.0, 255.0) as i32;
        poly.push(Point::new(lon_px, lat_px));
    }
    poly.pop();

    let tiledata = reqwest::blocking::get(path)
        .unwrap()
        .bytes()
        .unwrap()
        .to_vec();
    //    let mut img = ImageBuffer::new(256, 256);
    let mut img = image::io::Reader::new(Cursor::new(tiledata))
        .with_guessed_format()
        .unwrap()
        .decode()
        .unwrap();

    let mut poly = vec![];
    for coord in coords.iter() {
        let min_lat = tile.top() as f64;
        let max_lat = tile.bottom() as f64;
        let (max_lat, min_lat) = (min_lat.min(max_lat), max_lat.max(min_lat));
        let min_lon = tile.left() as f64;
        let max_lon = tile.right() as f64;
        let (min_lon, max_lon) = (min_lon.min(max_lon), max_lon.max(min_lon));

        let lon_px = translate(coord.0, min_lat, max_lat, 0.0, 255.0) as i32;
        let lat_px = translate(coord.1, min_lon, max_lon, 0.0, 255.0) as i32;
        poly.push(Point::new(lat_px, lon_px));
    }
    poly.pop();

    imageproc::drawing::draw_polygon_mut(&mut img, &poly, image::Rgba([0u8, 255, 0, 128]));

    img.save(format!("way-{}-{}-{}.png", way.id.0, tile.y(), tile.x()))
        .unwrap();
}

fn fetch_outline_relation(rel: &Relation, nodes: &HashMap<i64, Node>, ways: &HashMap<i64, Way>) {
    println!("{rel:?}");
    for wayref in rel.refs.iter().filter(|v| v.member.is_way()) {
        println!("Has wayref: {wayref:?}");
        let wayid = wayref.member.inner_id();
        if let Some(way) = ways.get(&wayid) {
            println!("Found way: {way:?}");
            fetch_outline_way(way, nodes);
        } else {
            println!("This way not found");
        }
    }
}

fn build_outlines(filename: &std::ffi::OsStr) {
    println!("Loading...");
    let r = std::fs::File::open(&std::path::Path::new(filename)).unwrap();
    let len = r.metadata().unwrap().len();
    let r = ProgressFile::new(r, len);
    let mut pbf = osmpbfreader::OsmPbfReader::new(r);

    let mut nodes_all = HashMap::new();
    let mut nodes_only_buildings = HashMap::new();
    let mut ways_all = HashMap::new();
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
                    ways_buildings.insert(way.id.0, way.clone());
                }
                ways_all.insert(way.id.0, way);
            }
            osmpbfreader::OsmObj::Relation(rel) => {
                if is_building {
                    relations_buildings.insert(rel.id.0, rel);
                }
            }
        }
    }
    println!("Loaded!");

    for way in ways_buildings.iter().take(10) {
        println!("{way:?}");
        fetch_outline_way(way.1, &nodes_all);
    }

    // for rel in relations_buildings.iter().take(50) {
    //     println!("------------");
    //     fetch_outline(rel.1, &nodes_all, &ways_all);
    // }
}

fn main() {
    env_logger::init();
    let file =
        //&OsString::from_str("/home/danya/Downloads/central-fed-district-latest.osm.pbf").unwrap();
        &OsString::from_str("/home/danya/Downloads/kaliningrad-latest.osm.pbf").unwrap();
    // fetch_buildings(&file);
    build_outlines(&file);
}
