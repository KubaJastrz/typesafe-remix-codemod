mod codemod;
mod fixer;
mod utils;

use oxc_span::SourceType;

use serde_json::Value;
use spinners::{Spinner, Spinners};
use std::{fs, process};

fn main() {
    let resolved_dir = utils::get_resolved_dir();

    println!("Working directory: {}", resolved_dir);

    let mut spinner = Spinner::new(Spinners::Dots, "Gathering route files...".into());
    let routes_raw = utils::get_remix_routes_json(&resolved_dir);
    spinner.stop_with_symbol("\x1b[32m✓\x1b[0m");

    let routes_json: Value = serde_json::from_str(&routes_raw).expect("Failed to parse JSON");

    if let Some(array) = routes_json.as_array() {
        let file_paths: Vec<String> = array
            .iter()
            .flat_map(|item| {
                let relative_files = utils::traverse_route_entry(item.clone());
                utils::get_absolute_files(relative_files, &resolved_dir)
            })
            .collect();

        println!("Found {} route files", file_paths.len());

        for file_path in file_paths.iter() {
            process_file(&file_path);
        }
    } else {
        eprintln!("Failed to parse JSON: expected an array");
        process::exit(1)
    }
}

fn process_file(file_path: &str) {
    println!("Processing file: {}", file_path);

    let source_text = fs::read_to_string(file_path).unwrap();
    let source_type = SourceType::from_path(file_path).unwrap();

    let fixed_code = codemod::codemod(&source_text, source_type);

    if let Err(_) = fixed_code {
        println!("Failed to process file: {}", file_path);
        process::exit(1);
    }

    fs::write(file_path, fixed_code.unwrap()).expect("Failed to write file");
}
