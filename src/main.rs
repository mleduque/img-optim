
use anyhow::{anyhow, bail};
use anyhow::{Result};
use clap::{Parser};
use dialoguer::Confirm;
use env_logger::{Env, Target};
use globwalk::{FileType as GlobFileType, glob_builder};
use lazy_static::lazy_static;
use log::{info, debug, warn, error};
use path_absolutize::*;
use regex::{Regex, escape};
use std::fs::{create_dir_all};
use std::path::{Path};
use std::process::Command;

#[derive(Parser, Clone)]
#[command(version = "1.0", author = "Mickaël Leduque <mleduque@gmail.com>")]
struct Opts {
    source: String,
    target: String,
    #[clap(long, short)]
    quality: Option<String>,
    #[clap(long, short)]
    geometry: Option<String>,
    #[clap(long, short)]
    define: Option<String>,
    #[clap(long, short)]
    extension: Option<String>,
    #[clap(long, short)]
    many: Option<String>,
}

fn main() -> Result<()> {
    env_logger::Builder::from_env(Env::default().default_filter_or("debug"))
                            .target(Target::Stdout)
                            .init();

    let opts: Opts = Opts::parse();
    match &opts.many {
        Some(pattern) => {
            let parts = resolve_pattern(&opts, pattern)?;
            for part in &parts {
                println!("{} => {}", part.source, part.target);
            }

            if Confirm::new().with_prompt("Do you want to continue?").interact()? {
                println!("Processing archives...");
                for part in parts.iter() {
                    process_archive(&part)?;
                }
            }
            Ok(())
        },
        None => process_archive(&opts),
    }
}

fn resolve_pattern(opts:&Opts, pattern: &str) -> Result<Vec<Opts>> {
    let pattern_len = pattern.len();
    let glob_pattern = if !opts.source.contains(pattern) {
        bail!("source name {} doesn't contain pattern {}",opts.source, pattern);
    } else {
        let escaped = escape_glob(&opts.source);
        let glob = escaped.replace(pattern, &"?".repeat(pattern_len));
        debug!("using glob '{}'", glob);
        glob
    };
    let source_regex = if !opts.source.contains(pattern) {
        bail!("source name {} doesn't contain pattern {}",opts.source, pattern);
    } else {
        let regex_pattern = escape(&opts.source).replace(pattern, &format!("(.{{{}}})", pattern_len));
        debug!("using pattern '{}'", regex_pattern);
        Regex::new(&regex_pattern)?
    };

    if !opts.target.contains(pattern) {
        bail!("target name {} doesn't contain pattern {}",opts.target, pattern);
    }

    let walker = glob_builder(glob_pattern)
        .file_type(GlobFileType::FILE )
        .sort_by(|a, b| a.path().to_str().unwrap().cmp(b.path().to_str().unwrap()))
        .build()?
        .into_iter()
        .filter_map(Result::ok);

    let mut result = vec![];
    for entry in walker {
        let path = entry.path().as_os_str().to_str().unwrap();
        debug!("file: {}", path);
        match source_regex.captures(path) {
            None => bail!("no idea what happened"),
            Some(captures) => {
                let capture = captures.get(1).unwrap();
                let value = capture.as_str();
                let target_name = opts.target.replace(pattern, value);
                result.push(Opts {
                    source: path.to_string(),
                    target: target_name,
                    many: None,
                    ..opts.clone()
                });
            }
        }
    }
    Ok(result)
}

fn process_archive(opts: &Opts)-> Result<()> {
    info!("creating temp dirs");
    let unpack_dir = tempfile::Builder::new().prefix("img-optim-unpack").tempdir()?;
    let processed_dir = tempfile::Builder::new().prefix("img-optim-uprocessed").tempdir()?;
    info!("temp dirs created [unpack_dir={:?} processed_dir={:?}]", unpack_dir, processed_dir);

    let target_zip = Path::new(&opts.target).absolutize()?;
    info!("target zip path: {:?}",target_zip);

    info!("start unpacking");
    unpack_archive(&Path::new(&opts.source).absolutize()?.to_owned(), &unpack_dir)?;
    info!("unpacking done");

    info!("start processing files");
    process_files(&unpack_dir.path(), &processed_dir.path(), &opts)?;
    info!("processing done");

    info!("start zipping output");
    let result = repack_output(&processed_dir, &target_zip);
    info!("zipping done");
    result
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
        let full_out_path = tmp_dir.path().join(&out_path);
        debug!("unpack {:?} to {:?}", out_path, full_out_path);

        if (&*file.name()).ends_with('/') {
            debug!("create dir {:?}", full_out_path);
            std::fs::create_dir_all(full_out_path)?;
        } else {
            create_parent(&full_out_path)?;
            let mut out_file = std::fs::File::create(full_out_path)?;
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

fn process_files(source: &dyn AsRef<Path>, target: &Path, opts: &Opts) -> Result<()> {
    let walker = globwalk::GlobWalkerBuilder::from_patterns(
        source,
        &[ "**/*" ],
    )
    .file_type(GlobFileType::FILE | GlobFileType::DIR)
    .contents_first(false) // directory before content
    .build()?
    .into_iter()
    .filter_map(Result::ok);

    for entry in walker {
        let entry_type = entry.file_type();
        debug!("{:?} type {:?}", entry, entry_type);
        if entry_type.is_dir() {
            // create dir in destination
            let path = entry.path();
            match path.absolutize() {
                Ok(canon) => {
                    info!("create directory {:?}", canon);
                    create_dir_all(canon)?
                },
                Err(error) => {
                    warn!("couldn't absolutize path {:?} - {:?}", entry, error);
                }
            }
        } else if entry_type.is_file() {
            // process file
            let path = entry.path();
            match path.absolutize() {
                Ok(canon) => match process_one_file(&canon, source.as_ref(), target, opts) {
                    Ok(_) => {}
                    Err(error) => {
                        error!("{}", error);
                        // continue with other files
                    }
                }
                Err(error) => {
                    warn!("couldn't absolutize path {:?} - {:?}", entry, error);
                }
            }
        } else {
            println!("{:?} is not a file or directory, skipping", entry);
        }

    }
    Ok(())
}

lazy_static! {
    static ref IMAGE_EXTENSIONS: Vec<&'static str> = vec!["jpg", "png", "webp", "avif", "gif"];
}

fn process_one_file(item: &Path, source: &Path, target: &Path, opts: &Opts) -> Result<()> {
    let extension = item.extension()
                .map_or_else(
                    || "".to_string(),
                    |ext| ext.to_str().unwrap_or("").to_string()
                );
    if IMAGE_EXTENSIONS.contains(&extension.as_str()) {
        process_one_image(item, source, target, opts)
    } else {
        let sub_path = item.strip_prefix(source)?;
        let destination = target.join(sub_path);
        let _ = std::fs::copy(item, destination);
        Ok(())
    }
}

fn process_one_image(item: &Path, source: &Path, target: &Path, opts: &Opts) -> Result<()> {
    let sub_path = item.strip_prefix(source)?;

    let result = target.join(sub_path)
        .with_extension(&opts.extension.as_deref()
        .unwrap_or("jpg"));
    create_parent(&result)?;

    let mut args: Vec<String> = vec![
        "convert".to_string(), item.as_os_str().to_str().unwrap().to_string(),
        "-geometry".to_string(), opts.geometry.as_deref().unwrap_or("1000x1400^").to_string(),
        "-quality".to_string(), opts.quality.as_deref().unwrap_or("80").to_string(),
    ];

    if let Some(define) = &opts.define {
        args.push("define".to_string());
        args.push(define.to_string());
    }
    args.push(result.to_str().unwrap().to_string());

    let mut command = Command::new("gm");
    command.args(args);
    debug!("Command: {:?}", command);

    let output = command.output()?;
    if output.status.success() {
        Ok(())
    } else {
        let error = format!("`gm convert` invocation failed\n{}\n",
        String::from_utf8_lossy(&output.stderr));
        Err(anyhow!(error))
    }
}

fn repack_output(dir: &tempfile::TempDir, zip: &Path) -> Result<()> {
    let zip_path= zip.to_str().unwrap();
    let mut command = Command::new("zip");
    command.args(vec![
        "--recurse-paths",
        zip_path, // zip file
        "." // what to add
    ]);
    command.current_dir(dir);

    let output = command.output()?;
    if output.status.success() {
        Ok(())
    } else {
        let error = format!("`zip` invocation failed\n{}\n",
        String::from_utf8_lossy(&output.stderr));
        Err(anyhow!(error))
    }
}

fn create_parent(file_path: &Path) -> Result<()> {
    if let Some(parent) = file_path.parent() {
        if !parent.exists() {
            debug!("create directory {:?}", parent);
        }
        std::fs::create_dir_all(parent)?;
    }
    Ok(())
}

fn escape_glob(input: &str) -> String {
    input.replace("!", r#"\!"#)
        .replace("#", r#"\#"#)
        .replace("*", r#"\*"#)
        .replace("?", r#"\?"#)
        .replace("#", r#"\#"#)
        .replace("[", r#"\["#)
        .replace("]", r#"\]"#)
        .replace("{", r#"\{"#)
        .replace("}", r#"\}"#)
}
