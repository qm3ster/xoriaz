use bip39::{Language::English, Mnemonic};
use clap::{Args, Parser, Subcommand};
use rand::{rngs::OsRng, Fill};
use smallvec::{smallvec, SmallVec};
use std::{
    fs::{File, OpenOptions},
    io::{stdout, BufRead, BufReader, BufWriter, Write},
    iter::zip,
    path::{Path, PathBuf},
};

#[derive(Debug, Parser)]
struct Cli {
    #[clap(subcommand)]
    command: Command,
}
#[derive(Debug, Subcommand)]
enum Command {
    /// Generate new random files
    Gen(Gen),
    /// Split source file into provided number of output files
    Split(Split),
    /// XOR source files together
    ///
    /// use `-o`/`--out` to specify an output file
    Xor(Xor),
}
#[derive(Debug, Args)]
struct Gen {
    #[clap(short, long, default_value = "200")]
    lines: usize,
    #[clap(parse(from_os_str))]
    dest: Vec<PathBuf>,
}
#[derive(Debug, Args)]
struct Split {
    #[clap(parse(from_os_str))]
    source: PathBuf,
    #[clap(required = true, min_values = 2, parse(from_os_str))]
    dest: Vec<PathBuf>,
}
#[derive(Debug, Args)]
struct Xor {
    #[clap(required = true, min_values = 2, parse(from_os_str))]
    source: Vec<PathBuf>,
    #[clap(short = 'o', long = "out", parse(from_os_str))]
    dest: Option<PathBuf>,
}

fn main() {
    match Cli::parse().command {
        Command::Gen(args) => gen(args),
        Command::Split(args) => split(args),
        Command::Xor(args) => xor(args),
    }
}

fn gen(Gen { lines, dest }: Gen) {
    let new_files = create_files(&dest);
    if new_files.is_empty() {
        gen_inner(stdout().lock(), lines)
    }
    for file in new_files {
        gen_inner(BufWriter::new(file), lines);
    }
}

fn create_file(path: &Path) -> BufWriter<File> {
    BufWriter::new(
        OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(path)
            .expect("btw, I won't overwrite"),
    )
}

fn create_files(paths: &[PathBuf]) -> SmallVec<[File; 4]> {
    let mut files = SmallVec::with_capacity(paths.len());
    let mut opts = OpenOptions::new();
    opts.create_new(true).write(true);
    for path in paths {
        match opts.open(path) {
            Ok(file) => files.push(file),
            Err(err) => {
                for (file, path) in zip(files, paths) {
                    drop(file);
                    if let Err(err) = std::fs::remove_file(path) {
                        eprintln!("{err}");
                    };
                }
                panic!("btw, I won't overwrite: {err}");
            }
        }
    }
    files
}

fn open_file(path: &Path) -> BufReader<File> {
    BufReader::new(
        OpenOptions::new()
            .read(true)
            .open(path)
            .expect("couldn't open for reading"),
    )
}

fn open_files(paths: &[PathBuf]) -> SmallVec<[BufReader<File>; 4]> {
    let mut files = SmallVec::with_capacity(paths.len());
    let mut opts = OpenOptions::new();
    opts.read(true);
    for path in paths {
        files.push(BufReader::new(
            opts.open(path).expect("couldn't open for reading"),
        ))
    }
    files
}

fn gen_inner(mut w: impl Write, lines: usize) {
    let rng = &mut OsRng;
    let mut buf = [0u8; 32];
    for _ in 0..lines {
        buf.try_fill(rng).unwrap();
        writeln!(&mut w, "{}", Mnemonic::from_entropy(&buf).unwrap()).unwrap();
    }
}

fn split(Split { source, dest }: Split) {
    let mut source = open_file(&source);
    let mut new_files = create_files(&dest);
    let (first, rest) = new_files.split_first_mut().expect("cli checked");
    let mut str = String::new();
    let mut bufs: SmallVec<[[u8; 32]; 3]> = smallvec![[0; 32]; rest.len()];
    let mut i = 0;
    let rng = &mut OsRng;
    while source.read_line(&mut str).unwrap() != 0 {
        let src = match Mnemonic::parse_in_normalized(English, &str) {
            Ok(src) => src,
            Err(e) => panic!("error on line {i}: {e}"),
        };
        str.clear();
        i += 1;
        let (mut src, len) = src.to_entropy_array();
        assert_eq!(len, 32);
        let src: &mut [u8; 32] = (&mut src[..32]).try_into().unwrap();
        for (buf, file) in zip(&mut bufs, &mut *rest) {
            buf.try_fill(rng).unwrap();
            for (s, b) in zip(&mut *src, &*buf) {
                *s ^= b
            }
            writeln!(file, "{}", Mnemonic::from_entropy(buf).unwrap()).unwrap();
        }
        writeln!(first, "{}", Mnemonic::from_entropy(src).unwrap()).unwrap();
    }
}

fn xor(Xor { source, dest }: Xor) {
    let mut inputs = open_files(&source);
    if let Some(path) = dest {
        xor_inner(create_file(&path), &mut inputs);
    } else {
        xor_inner(stdout().lock(), &mut inputs);
    }
}

fn xor_inner(mut w: impl Write, inputs: &mut [BufReader<File>]) {
    let buf = &mut [0u8; 32];
    let str = &mut String::new();
    let mut i = 0;
    let mut f;
    let mut finishing = false;
    while !finishing {
        f = 0;
        for file in &mut *inputs {
            let len = file.read_line(str).expect("couldn't read");
            if len == 0 {
                if f == 0 {
                    finishing = true
                } else if !finishing {
                    panic!("file {f} suddenly ended")
                }
                continue;
            } else if finishing {
                panic!("file {f} continues longer than previous file")
            }
            let m = match Mnemonic::parse_in_normalized(English, str) {
                Ok(m) => m,
                Err(e) => panic!("error on line {i} in file {f}: {e}"),
            };
            str.clear();
            let (m, len) = m.to_entropy_array();
            assert_eq!(len, 32);
            let m: &[u8; 32] = (&m[..32]).try_into().unwrap();
            for (s, b) in zip(&mut *buf, m) {
                *s ^= b
            }
            f += 1;
        }
        writeln!(w, "{}", Mnemonic::from_entropy(buf).unwrap()).unwrap();
        *buf = [0; 32];
        i += 1;
    }
}
