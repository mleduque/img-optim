
use anyhow::anyhow;
use anyhow::{Result};
use clap::Clap;
use glob::glob;
use path_absolutize::*;
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
    let dir = tempfile::tempdir()?;
    let out_dir = tempfile::tempdir()?;
    println!("temp dirs ok");
    let target_zip = Path::new(&opts.target).absolutize()?;
    println!("absolutize ok");

    unpack_archive(&Path::new(&opts.source).absolutize()?.to_owned(), &dir)?;
    let pattern = match dir.path().to_str() {
        Some(path) => path.to_owned() + "/*",
        None => {
            println!("invalid temp file path {:?}", dir.path());
            return Err(anyhow!("invalid temp file path"));
        }
    };
    println!("unpack ok");
    process_files(&pattern, &out_dir.path())?;
    println!("process ok");
    repack_output(&out_dir, &target_zip)
}

fn unpack_archive(zip_path: &Path, tmp_dir: &tempfile::TempDir) -> Result<()> {
    let zip_file = std::fs::File::open(&zip_path)?;
    let mut archive = zip::ZipArchive::new(zip_file)?;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let out_path = match file.enclosed_name() {
            Some(path) => path.to_owned(),
            None => return Err(anyhow!("invalid name for file in archive: {:?}", file.mangled_name())),
        };

        if (&*file.name()).ends_with('/') {
            std::fs::create_dir_all(tmp_dir.path().join(&out_path))?;
        } else {
            if let Some(p) = out_path.parent() {
                if !p.exists() {
                    std::fs::create_dir_all(tmp_dir.path().join(&p))?;
                }
            }
            let mut out_file = std::fs::File::create(tmp_dir.path().join(&out_path))?;
            std::io::copy(&mut file, &mut out_file)?;
        }
                // Get and Set permissions
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            if let Some(mode) = file.unix_mode() {
                std::fs::set_permissions(tmp_dir.path().join(&out_path), std::fs::Permissions::from_mode(mode))?;
            }
        }
    }
    Ok(())
}

fn optimize_images(source_glob: &str, target_dir: &str) -> Result<()> {
    
    let target = Path::new(target_dir);
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

    process_files(source_glob, target)
}

fn process_files(source_glob: &str, target: &Path) -> Result<()> {
    for entry in glob(&source_glob)? {
        println!("{:?}", entry);
        match entry {
            Ok(ref path) => {
                if path.is_file() {
                    match path.absolutize() {
                        Ok(canon) => match process_one_file(&canon, &target) {
                                        Ok(_) => {}
                                        Err(error) => {
                                            println!("{}", error);
                                            // continue with other files
                                        }
                                    }
                        Err(error) => {
                            println!("couldn't absolutize path {:?} - {:?}", entry, error);
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
fn repack_output(dir: &tempfile::TempDir, zip: &Path) -> Result<()> {
    use zip::CompressionMethod;
    let options = zip::write::FileOptions::default().compression_method(CompressionMethod::Stored);
    zip_extensions::write::zip_create_from_directory_with_options(&zip.to_owned(), &dir.path().to_owned(), options)?;
    Ok(())
}