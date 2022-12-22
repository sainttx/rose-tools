use std::f32;
use std::fs;
use std::fs::File;
use std::io::{Read, Write};
use std::iter;
use std::path::{Path, PathBuf};
use std::process::exit;

use clap::{App, AppSettings, Arg, ArgMatches, crate_authors, crate_version, SubCommand};
use failure::{bail, Error};
use image::{GrayImage, ImageBuffer, RgbaImage};
use image::io::Reader as ImageReader;
use serde::{Deserialize, Serialize};

use rose_conv::{ToObj};
use rose_conv::{FromCsv, ToCsv};
use rose_conv::{FromJson, ToJson};
use roselib::files::*;
use roselib::files::zon::ZoneTileRotation;
use roselib::io::{RoseFile, RoseReader};

const SERIALIZE_VALUES: [&'static str; 14] = [
    "him", "idx", "ifo", "lit", "stb", "stl", "wstb", "til", "tsi", "zmd", "zmo", "zms", "zon",
    "zsc",
];

const DESERIALIZE_VALUES: [&'static str; 5] = ["idx", "lit", "stb", "stl", "zsc"];

#[derive(Debug, Deserialize, Serialize)]
struct TilemapTile {
    layer1: i32,
    layer2: i32,
    rotation: ZoneTileRotation,
}

#[derive(Debug, Deserialize, Serialize)]
struct TilemapFile {
    textures: Vec<String>,
    tiles: Vec<TilemapTile>,
    tilemap: Vec<Vec<i32>>,
}

fn main() {
    let matches = App::new("ROSE Converter")
        .version(crate_version!())
        .author(crate_authors!())
        .about("Convert ROSE Online files to/from various formats")
        .arg(
            Arg::with_name("out_dir")
                .help("Directory to output converted files")
                .default_value("./out/")
                .short("o")
                .global(true),
        )
        .settings(&[
            AppSettings::SubcommandRequiredElseHelp,
            AppSettings::VersionlessSubcommands,
            AppSettings::DeriveDisplayOrder,
        ])
        .subcommand(
            SubCommand::with_name("map")
                .about("Convert ROSE map files")
                .arg(
                    Arg::with_name("map_dir")
                        .help("Map directory containing zon, him, til and ifo files")
                        .required(true),
                ),
        )
        .subcommand(
            SubCommand::with_name("iconsheet")
                .about("Convert ROSE iconsheet to icon files")
                .arg(
                    Arg::with_name("iconsheets")
                        .help("Path to iconsheet")
                        .required(true)
                        .multiple(true),
                ),
        )
        .subcommand(
            SubCommand::with_name("serialize")
                .visible_alias("se")
                .about("Serialize a ROSE File into JSON (CSV for STB/STL).")
                .arg(
                    Arg::with_name("input")
                        .help("Path to ROSE file")
                        .required(true),
                )
                .arg(
                    Arg::with_name("type")
                        .help("Type of file")
                        .required(false)
                        .short("t")
                        .long("type")
                        .takes_value(true)
                        .possible_values(&SERIALIZE_VALUES),
                )
                .arg(
                    Arg::with_name("keep-extension")
                        .long("keep-extension")
                        .help("Keep the original file extension in addition to the next one, e.g. list_zone.stb.csv")
                        .required(false)
                        .takes_value(false)
                ),
        )
        .subcommand(
            SubCommand::with_name("deserialize")
                .visible_alias("de")
                .about("Deserialize a ROSE file from JSON (CSV for STB/STL).")
                .arg(
                    Arg::with_name("type")
                        .help("ROSE file type")
                        .case_insensitive(true)
                        .possible_values(&DESERIALIZE_VALUES)
                        .required(true),
                )
                .arg(
                    Arg::with_name("input")
                        .help("Path to JSON/CSV file")
                        .required(true),
                )
                .arg(
                    Arg::with_name("output")
                        .help("Path to output file location (Optional)")
                        .long_help(
"Path to output file location (Optional). This will create a file at
the path location regardless of file extension. This option takes
priority over the output directory flag (`-o`)."
                        )
                        .conflicts_with("out_dir")
                )
                ,
        )
        .get_matches();

    // Run subcommands
    let res = match matches.subcommand() {
        ("map", Some(matches)) => convert_map(matches),
        ("serialize", Some(matches)) => serialize(matches),
        ("deserialize", Some(matches)) => deserialize(matches),
        ("iconsheet", Some(matches)) => convert_iconsheets(matches),
        _ => {
            eprintln!("ROSE Online Converter. Run with `--help` for more info.");
            exit(1);
        }
    };

    if let Err(e) = res {
        eprintln!("Error occured: {}", e);
        let filename = match matches.subcommand() {
            ("serialize", Some(matches)) => matches.value_of("input"),
            ("deserialize", Some(matches)) => matches.value_of("input"),
            _ => None,
        };

        if let Some(name) = filename {
            eprintln!("\t{}", name);
        }
    }
}

fn create_output_dir(out_dir: &Path) -> Result<(), Error> {
    if let Err(e) = fs::create_dir_all(&out_dir) {
        bail!(
            "Error creating output directory {}: {}",
            out_dir.to_str().unwrap_or(""),
            e
        );
    }
    Ok(())
}

fn serialize(matches: &ArgMatches) -> Result<(), Error> {
    let select = Path::new(matches.value_of("input").unwrap_or_default());

    if !select.exists() {
        bail!("File does not exist: {}", select.display());
    }

    if select.is_dir() {
        for entry in fs::read_dir(select)? {
            let path = entry?;
            serialize_in(matches, &path.path());
        }
    } else {
        return serialize_in(matches, select);
    }

    Ok(())
}

fn serialize_in(matches: &ArgMatches, input: &Path) -> Result<(), Error> {
    let out_dir = Path::new(matches.value_of("out_dir").unwrap_or_default());
    let input_type = matches.value_of("type").unwrap_or_default();

    let extension = input
        .extension()
        .unwrap_or_default()
        .to_str()
        .unwrap_or_default()
        .to_lowercase();

    let rose_type = if input_type.is_empty() {
        if !SERIALIZE_VALUES.contains(&extension.as_str()) {
            bail!("No type provided and unrecognized extension");
        }
        String::from(&extension)
    } else {
        String::from(input_type)
    };

    let data = match rose_type.as_str() {
        // CSV
        "stb" => STB::from_path(&input)?.to_csv()?,
        "stl" => STL::from_path(&input)?.to_csv()?,
        // OBJ
        "zms" => ZMS::from_path(&input)?.to_obj()?,
        // JSON
        "him" => HIM::from_path(&input)?.to_json()?,
        "idx" => IDX::from_path(&input)?.to_json()?,
        "ifo" => IFO::from_path(&input)?.to_json()?,
        "lit" => LIT::from_path(&input)?.to_json()?,
        "til" => TIL::from_path(&input)?.to_json()?,
        "tsi" => TSI::from_path(&input)?.to_json()?,
        "zmd" => ZMD::from_path(&input)?.to_json()?,
        "zmo" => ZMO::from_path(&input)?.to_json()?,
        "zon" => ZON::from_path(&input)?.to_json()?,
        "zsc" => ZSC::from_path(&input)?.to_json()?,
        "wstb" => {
            let f = File::open(input)?;
            let mut reader = RoseReader::new(f);
            reader.set_wide_strings(true);
            let mut stb: STB = RoseFile::new();
            stb.read(&mut reader)?;
            stb.to_csv()?
        }
        _ => bail!("Unsupported file type: {}", rose_type.as_str()),
    };

    let new_extension = if rose_type == "stb" || rose_type == "stl" {
        "csv"
    } else if rose_type == "zms" {
        "obj"
    } else {
        "json"
    };

    // If the keep-extension flag is present we prepend the original extension
    // e.g. list_zone.stb.json
    let new_extension = if matches.is_present("keep-extension") {
        extension + "." + new_extension
    } else {
        String::from(new_extension)
    };

    let out = out_dir
        .join(input.file_name().unwrap_or_default())
        .with_extension(new_extension);

    if let Some(p) = out.parent() {
        create_output_dir(p)?;
    }

    let mut f = File::create(&out)?;
    f.write_all(data.as_bytes())?;

    Ok(())
}

fn deserialize(matches: &ArgMatches) -> Result<(), Error> {
    let filetype = matches.value_of("type").unwrap_or_default();
    let input = Path::new(matches.value_of("input").unwrap_or_default());

    if !input.exists() {
        bail!("File does not exist: {}", input.display());
    }

    // Use the output arg if it's set, otherwise use the output directory option
    let out = if let Some(s) = matches.value_of("output") {
        PathBuf::from(s)
    } else {
        let out_dir = Path::new(matches.value_of("out_dir").unwrap_or_default());
        out_dir
            .join(input.file_name().unwrap_or_default())
            .with_extension(filetype)
    };

    if let Some(p) = out.parent() {
        create_output_dir(p)?;
    }

    let mut data = String::new();

    let mut file = File::open(&input)?;
    file.read_to_string(&mut data)?;

    match filetype {
        "stb" => STB::from_csv(&data)?.write_to_path(&out)?,
        "stl" => STL::from_csv(&data)?.write_to_path(&out)?,
        "idx" => IDX::from_json(&data)?.write_to_path(&out)?,
        "lit" => IDX::from_json(&data)?.write_to_path(&out)?,
        "zsc" => IDX::from_json(&data)?.write_to_path(&out)?,
        _ => bail!("Unsupported file type: {}", filetype),
    }

    Ok(())
}

/// Convert map files:
/// - ZON: JSON
/// - TIL: Combined into 1 JSON file
/// - IFO: Combined into 1 JSON file
/// - HIM: Combined into 1 greyscale png
fn convert_map(matches: &ArgMatches) -> Result<(), Error> {
    let map_dir = Path::new(matches.value_of("map_dir").unwrap());
    if !map_dir.is_dir() {
        bail!("Map path is not a directory: {:?}", map_dir);
    }

    println!("Loading map from: {}", map_dir.to_str().unwrap());

    // Collect coordinates from file names (using HIM as reference)
    let mut x_coords: Vec<u32> = Vec::new();
    let mut y_coords: Vec<u32> = Vec::new();

    for f in fs::read_dir(map_dir)? {
        let f = f?;
        let fpath = f.path();
        if !fpath.is_file() {
            continue;
        }

        if fpath.extension().unwrap().to_str().unwrap().to_lowercase() == "him" {
            let fname = fpath.file_stem().unwrap().to_str().unwrap();
            let parts: Vec<&str> = fname.split('_').collect();
            x_coords.push(parts[0].parse()?);
            y_coords.push(parts[1].parse()?);
        }
    }

    x_coords.sort();
    y_coords.sort();

    let x_min = *x_coords.iter().min().unwrap();
    let x_max = *x_coords.iter().max().unwrap();
    let y_min = *y_coords.iter().min().unwrap();
    let y_max = *y_coords.iter().max().unwrap();

    let map_width = (x_max - x_min + 1) * 65;
    let map_height = (y_max - y_min + 1) * 65;

    let mut max_height = f32::NAN;
    let mut min_height = f32::NAN;

    // Ensure map dimensions are divisible by 4 for tiling
    let new_map_width = (map_width as f32 / 4.0).ceil() * 4.0;
    let new_map_height = (map_height as f32 / 4.0).ceil() * 4.0;

    let new_map_width = new_map_width as u32 + 1;
    let new_map_height = new_map_height as u32 + 1;

    let mut heights: Vec<Vec<f32>> = Vec::new();
    heights.resize(
        new_map_height as usize,
        iter::repeat(0.0).take(new_map_width as usize).collect(),
    );

    // Number of tiles in x and y direction
    let tiles_x = new_map_width / 4;
    let tiles_y = new_map_height / 4;

    let mut tiles: Vec<Vec<i32>> = Vec::new();
    tiles.resize(
        tiles_y as usize,
        iter::repeat(0).take(tiles_x as usize).collect(),
    );

    for y in y_min..=y_max {
        for x in x_min..=x_max {
            //-- Load HIMs
            let him_name = format!("{}_{}.HIM", x, y);
            let him_path = map_dir.join(&him_name);

            let him = HIM::from_path(&him_path).unwrap();
            if him.length != 65 || him.width != 65 {
                bail!(
                    "Unexpected HIM dimensions. Expected 65x65: {} ({}x{})",
                    &him_path.to_str().unwrap_or(&him_name),
                    him.width,
                    him.length
                );
            }

            for h in 0..him.length {
                for w in 0..him.width {
                    let height = him.height(h as usize, w as usize);

                    if (height > max_height) || (max_height.is_nan()) {
                        max_height = height;
                    }
                    if (height < min_height) || (min_height.is_nan()) {
                        min_height = height;
                    }

                    let new_x = ((x - x_min) * 65) + w as u32;
                    let new_y = ((y - y_min) * 65) + h as u32;

                    heights[new_y as usize][new_x as usize] = height;
                }
            }

            // -- Load TILs
            let til_name = format!("{}_{}.TIL", x, y);
            let til_path = map_dir.join(&til_name);

            let til = TIL::from_path(&til_path).unwrap();
            if til.height != 16 || til.width != 16 {
                bail!(
                    "Unexpected TIL dimensions. Expected 16x16: {} ({}x{})",
                    &til_path.to_str().unwrap_or(&til_name),
                    til.width,
                    til.height
                );
            }

            for h in 0..til.height {
                for w in 0..til.width {
                    let tile_id = til.tiles[h as usize][w as usize].tile_id;

                    let new_x = ((x - x_min) * 16) + w as u32;
                    let new_y = ((y - y_min) * 16) + h as u32;

                    tiles[new_y as usize][new_x as usize] = tile_id;
                }
            }

            // TODO:
            // Load IFO data
        }
    }

    let map_name = map_dir.file_name().unwrap().to_str().unwrap();
    let out_dir = Path::new(matches.value_of("out_dir").unwrap_or("out"));
    create_output_dir(out_dir)?;

    // -- Heightmap image
    let delta_height = max_height - min_height;

    let mut height_image: GrayImage = ImageBuffer::new(new_map_width, new_map_height);

    for y in 0..new_map_height {
        for x in 0..new_map_width {
            let height = heights[y as usize][x as usize];

            let norm_height = |h| (255.0 * ((h - min_height) / delta_height)) as u8;

            let pixel = image::Luma([norm_height(height)]);
            height_image.put_pixel(x, y, pixel);
        }
    }

    // Save heightmap image
    let mut height_file = PathBuf::from(out_dir);
    height_file.push(map_name);
    height_file.set_extension("png");

    println!("Saving heightmap to: {}", &height_file.to_str().unwrap());
    height_image.save(height_file)?;

    // Dump ZON as JSON
    let zon = ZON::from_path(&map_dir.join(format!("{}.ZON", map_name)))?;
    let mut zon_file = PathBuf::from(out_dir);
    zon_file.push(map_name.to_string());
    zon_file.set_extension("json");

    println!("Dumping ZON file to: {}", &zon_file.to_str().unwrap());
    let f = File::create(zon_file)?;
    serde_json::to_writer_pretty(f, &zon)?;

    // Create tilemap file
    let mut tilemap_tiles: Vec<TilemapTile> = Vec::new();
    for zon_tile in zon.tiles {
        tilemap_tiles.push(TilemapTile {
            layer1: zon_tile.layer1 + zon_tile.offset1,
            layer2: zon_tile.layer2 + zon_tile.offset2,
            rotation: zon_tile.rotation,
        });
    }

    let tilemap = TilemapFile {
        textures: zon.textures,
        tiles: tilemap_tiles,
        tilemap: tiles,
    };

    let mut tile_file = PathBuf::from(out_dir);
    tile_file.push(format!("{}_tilemap", map_name));
    tile_file.set_extension("json");

    println!("Saving tilemap file to: {}", &tile_file.to_str().unwrap());
    let f = File::create(tile_file)?;
    serde_json::to_writer_pretty(f, &tilemap)?;

    // EXPORT IFO data as JSON

    Ok(())
}

fn convert_iconsheets(matches: &ArgMatches) -> Result<(), Error> {
    let out_dir = Path::new(matches.value_of("out_dir").unwrap_or_default());
    let iconsheet_paths: Vec<PathBuf> = matches
        .values_of("iconsheets")
        .unwrap_or_default()
        .map(|p| PathBuf::from(p))
        .collect();

    let convert_iconsheet = |iconsheet_path: &Path| -> Result<(), Error> {
        if !iconsheet_path.exists() {
            bail!("File does not exist: {}", iconsheet_path.display());
        }

        let img = ImageReader::open(iconsheet_path)?.decode()?.into_rgba8();

        // ROSE Icons are 40 pixels x 40 pixels
        let icon_x_count = (img.width() as f32 / 40.0).floor() as u32;
        let icon_y_count = (img.height() as f32 / 40.0).floor() as u32;

        let mut icon_number = 0;
        for icon_y in 0..icon_y_count {
            for icon_x in 0..icon_x_count {
                let mut icon = RgbaImage::new(40, 40);

                for pixel_y in 0..40 {
                    for pixel_x in 0..40 {
                        let x = (icon_x * 40) + pixel_x;
                        let y = (icon_y * 40) + pixel_y;
                        let pixel = img.get_pixel(x, y);
                        icon.put_pixel(pixel_x, pixel_y, *pixel);
                    }
                }

                let icon_name = iconsheet_path.file_stem().unwrap();
                let icon_path = out_dir
                    .join(format!("{}_{}", icon_name.to_str().unwrap(), icon_number))
                    .with_extension("png");
                dbg!(&icon_path);
                icon.save(&icon_path)?;

                icon_number += 1;
            }
        }

        Ok(())
    };

    create_output_dir(out_dir)?;

    let mut all_succeeded = true;
    for iconsheet_path in iconsheet_paths {
        if let Err(e) = convert_iconsheet(&iconsheet_path) {
            all_succeeded = false;
            eprintln!("{}", e);
        }
    }

    if !all_succeeded {
        bail!("Failed to convert all tilesheets");
    }

    eprintln!("Done.");
    Ok(())
}

/*
fn zms_to_obj(input: File, output: File) -> Result<(), Error> {
    let mut writer = BufWriter::new(output);

    //let z = ZMS::from_reader(&mut reader)?;
    let z = ZMS::from_file(&input)?;

    writer
        .write(format!("# Exported using {} v{} ({})\n",
                       env!("CARGO_PKG_NAME"),
                       env!("CARGO_PKG_VERSION"),
                       env!("CARGO_PKG_HOMEPAGE"))
                       .as_bytes())?;

    // -- Write vertex data
    for v in &z.vertices {
        writer
            .write(format!("v {} {} {}\n", v.position.x, v.position.y, v.position.z).as_bytes())?;
    }

    for v in &z.vertices {
        writer
            .write(format!("vt {} {}\n", v.uv1.x, 1.0 - v.uv1.y).as_bytes())?;
    }

    for v in &z.vertices {
        writer
            .write(format!("vn {} {} {}\n", v.normal.x, v.normal.y, v.normal.z).as_bytes())?;
    }

    // -- Write face data
    for i in z.indices {
        writer
            .write(format!("f {x}/{x}/{x} {y}/{y}/{y} {z}/{z}/{z}\n",
                           x = i.x + 1,
                           y = i.y + 1,
                           z = i.z + 1)
                           .as_bytes())?;
    }

    Ok(())
}
*/
