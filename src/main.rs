
use anyhow::anyhow;
use anyhow::{Result};
use clap::Clap;
use glob::glob;
use std::path::Path;
use std::process::Command;

#[derive(Clap)]
#[clap(version = "1.0", author = "Mickaël Leduque <mleduque@gmail.com>")]
struct Opts {
    source: String,
    target: String,
}

fn main() -> Result<()> {
    let opts: Opts = Opts::parse();
    
    let target = Path::new(&opts.target);
    if target.exists() {
        if target.is_dir() {
            // continue only if it's empty
            match target.read_dir() {
                Err(error) => {
                    println!("error reading target directory content ({}) - aborting", target.to_string_lossy());
                    return Err(anyhow!(error));
                }
                Ok(mut content) => {
                    if let Some(_) = content.next() {
                        println!("existing target path {} is not empty - aborting", target.to_string_lossy());
                        return Err(anyhow!("target path is not empty"));
                    }
                }
            }
        } else {
            println!("existing target path {} is not a directory - aborting", target.to_string_lossy());
            return Err(anyhow!("target path is not a directory"));
        }
    } else {
        std::fs::create_dir(target)?;
    }

    for entry in glob(&opts.source)? {
        println!("{:?}", entry);
        match entry {
            Ok(ref path) => {
                if path.is_file() {
                    match path.canonicalize() {
                        Ok(canon) => match process_one_file(&canon, &target) {
                                        Ok(_) => {}
                                        Err(error) => {
                                            println!("{}", error);
                                            // continue with other files
                                        }
                                    }
                        Err(error) => {
                            println!("couldn't canonicalize path {:?} - {:?}", entry, error);
                        }
                    }
                } else {
                    println!("{:?} is not a file, skipping", entry);
                }
            }
            Err(err) => {
                println!("{:?}", err);
            }
        }
    }
    Ok(())
}

fn process_one_file(item: &Path, target: &Path) -> Result<()> {
    if let Some(file_name) = item.file_name() {
        let result = target.join(file_name).with_extension("jpg");
        let output = Command::new("gm")
        .arg("convert")
        .arg(item.as_os_str())
        .arg("-geometry")
        .arg("800x1200^") // 1200 max width, 800 max height, using the biggest dimension ; the other is computed to keep aspect ratio
        .arg(&result)
        .output()?;
        if output.status.success() {
            Ok(())
        } else {
            let error = format!("`gm convert` invocation failed\n====\n{}\n===\n{}\n====", 
            String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
            Err(anyhow!(error))
        }
    } else {
        Err(anyhow!("path{} has no filename, skipping", item.to_string_lossy()))
    }
}