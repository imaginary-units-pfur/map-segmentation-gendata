use std::{collections::HashMap, ffi::OsString, str::FromStr};

fn fetch_buildings(filename: &std::ffi::OsStr) {
    let r = std::fs::File::open(&std::path::Path::new(filename)).unwrap();
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

fn main() {
    env_logger::init();
    let file =
        &OsString::from_str("/home/danya/Downloads/central-fed-district-latest.osm.pbf").unwrap();
    fetch_buildings(&file);
}
