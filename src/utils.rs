use serde_json::Value;
use std::env;
use std::path::Path;
use std::path::PathBuf;
use std::process::exit;
use std::process::Command;

pub fn get_absolute_files(files: Vec<String>, current_dir: &String) -> Vec<String> {
  files
      .iter()
      .map(|file| {
          let path = Path::new(&file);
          let absolute_path = PathBuf::from(current_dir).join("app").join(path);

          if let Ok(canonical_path) = absolute_path.canonicalize() {
              canonical_path.display().to_string()
          } else {
              eprintln!("Failed to resolve path: {}", absolute_path.display());
              exit(1)
          }
      })
      .collect()
}

pub fn traverse_route_entry(entry: Value) -> Vec<String> {
  let mut files = vec![];

  if let Some(obj) = entry.as_object() {
      for (key, value) in obj {
          if key == "file" {
              if let Some(file) = value.as_str() {
                  files.push(file.to_string());
              }
          }
          if key == "children" {
              if let Some(array) = value.as_array() {
                  for item in array {
                      for file in traverse_route_entry(item.clone()) {
                          files.push(file);
                      }
                  }
              }
          }
      }
  }

  files
}

pub fn get_remix_routes_json(current_dir: &String) -> String {
  let output = Command::new("npx")
      .arg("-y")
      .arg("@remix-run/dev")
      .arg("routes")
      .arg("--json")
      .current_dir(current_dir)
      .output()
      .expect("Failed to execute command");

  if output.status.success() {
      let files = String::from_utf8(output.stdout).unwrap();
      files
  } else {
      let error = String::from_utf8(output.stderr).unwrap();
      eprintln!("Error: {}", error);
      exit(1)
  }
}

pub fn get_resolved_dir() -> String {
  let args: Vec<String> = env::args().collect();
  let current_dir = env::current_dir().expect("Failed to get current directory");
  if args.len() > 1 {
      let path = Path::new(&args[1]);
      let absolute_path = if path.is_relative() {
          current_dir.join(path)
      } else {
          PathBuf::from(path)
      };

      if let Ok(canonical_path) = absolute_path.canonicalize() {
          canonical_path.display().to_string()
      } else {
          eprintln!("Failed to resolve path: {}", absolute_path.display());
          exit(1)
      }
  } else {
      current_dir.display().to_string()
  }
}
