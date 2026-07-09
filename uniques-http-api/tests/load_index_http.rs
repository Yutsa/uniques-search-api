use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};

use tar::{Builder, Header};
use uniques_http_api::{load_index_from_http, HttpIndexClient};

const FIXTURE_INDEX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/minimal_index");

fn collect_files(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut out = Vec::new();
    collect_files_rec(dir, dir, &mut out);
    out
}

fn collect_files_rec(root: &Path, dir: &Path, out: &mut Vec<(PathBuf, Vec<u8>)>) {
    for entry in fs::read_dir(dir).expect("read fixture dir") {
        let entry = entry.expect("read fixture entry");
        let path = entry.path();
        if path.is_dir() {
            collect_files_rec(root, &path, out);
        } else {
            let relative = path.strip_prefix(root).expect("strip fixture prefix");
            let bytes = fs::read(&path).expect("read fixture file");
            out.push((relative.to_path_buf(), bytes));
        }
    }
}

fn pack_fixture_to_bytes(fixture_dir: &Path) -> Vec<u8> {
    let mut tar_bytes = Vec::new();
    {
        let encoder = zstd::Encoder::new(&mut tar_bytes, 0).expect("create zstd encoder");
        let mut builder = Builder::new(encoder);

        for (relative, bytes) in collect_files(fixture_dir) {
            let archive_name = relative.to_string_lossy().replace('\\', "/");
            let mut header = Header::new_gnu();
            header.set_size(bytes.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder
                .append_data(&mut header, archive_name, &bytes[..])
                .expect("append tar entry");
        }

        let encoder = builder.into_inner().expect("finish tar");
        encoder.finish().expect("finish zstd");
    }
    tar_bytes
}

/// Serves `body` once to the first connection, as a plain unauthenticated GET
/// response -- mirroring a public object storage bucket.
fn serve_once(body: Vec<u8>) -> (String, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind local http listener");
    let addr = listener.local_addr().expect("local addr");

    let handle = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("accept connection");
        let mut buf = [0u8; 4096];
        let _ = stream.read(&mut buf);

        let header = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        stream.write_all(header.as_bytes()).expect("write header");
        stream.write_all(&body).expect("write body");
        stream.flush().expect("flush response");
    });

    (format!("http://{addr}/full_index.tar.zst"), handle)
}

#[test]
fn loads_minimal_fixture_from_http() {
    let archive_bytes = pack_fixture_to_bytes(Path::new(FIXTURE_INDEX));
    let (url, server) = serve_once(archive_bytes);

    let client = HttpIndexClient::new(url);
    let state = load_index_from_http(&client).expect("load from http");

    let index = state.index();
    assert_eq!(index.catalog().set, "TEST");
    assert_eq!(index.catalog().total_bit_span, 1);
    assert!(!index.effects_body().is_empty());

    server.join().expect("server thread panicked");
}
