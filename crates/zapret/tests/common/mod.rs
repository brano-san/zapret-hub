#![allow(dead_code)]

use std::fs;

/// A minimal Flowseal folder: what `Folder::open` checks, plus `utils\`.
pub fn fake_zapret() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::write(
        root.join("service.bat"),
        "@echo off\r\nset \"LOCAL_VERSION=1.10.3\"\r\n",
    )
    .unwrap();
    fs::create_dir_all(root.join("bin")).unwrap();
    fs::write(root.join("bin").join("winws.exe"), b"").unwrap();
    fs::write(root.join("bin").join("WinDivert64.sys"), b"").unwrap();
    fs::create_dir_all(root.join("lists")).unwrap();
    fs::create_dir_all(root.join("utils")).unwrap();
    dir
}

pub fn folder(dir: &tempfile::TempDir) -> zapret::folder::Folder {
    zapret::folder::Folder::open(dir.path()).unwrap()
}
