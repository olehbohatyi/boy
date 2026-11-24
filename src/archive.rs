//! Unpacking release files.

use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use anyhow::{Context, Result, bail};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    /// The release file is the executable itself.
    Bare,
    TarGz,
    TarXz,
    Zip,
}

impl Kind {
    pub fn of(file: &str) -> Self {
        if file.ends_with(".tar.gz") || file.ends_with(".tgz") {
            Kind::TarGz
        } else if file.ends_with(".tar.xz") || file.ends_with(".txz") {
            Kind::TarXz
        } else if file.ends_with(".zip") {
            Kind::Zip
        } else {
            Kind::Bare
        }
    }
}

/// Unpack `archive` into the existing folder `dest`. Entries that would land
/// outside `dest` (`..`, absolute paths) are refused by both readers.
pub fn extract(archive: &Path, kind: Kind, dest: &Path) -> Result<()> {
    let file = File::open(archive).with_context(|| format!("cannot read {}", archive.display()))?;
    let reader = BufReader::new(file);
    match kind {
        Kind::TarGz => {
            let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(reader));
            tar.set_preserve_permissions(true);
            tar.unpack(dest).context("cannot unpack .tar.gz")
        }
        Kind::Zip => {
            let mut zip = zip::ZipArchive::new(reader).context("cannot open .zip")?;
            for i in 0..zip.len() {
                let mut entry = zip.by_index(i)?;
                let Some(rel) = entry.enclosed_name() else {
                    bail!("unsafe path in .zip entry {i}");
                };
                let out = dest.join(rel);
                if entry.is_dir() {
                    std::fs::create_dir_all(&out)?;
                    continue;
                }
                if let Some(parent) = out.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                let mut f = File::create(&out)?;
                std::io::copy(&mut entry, &mut f)?;
                if let Some(mode) = entry.unix_mode() {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(&out, std::fs::Permissions::from_mode(mode & 0o777))?;
                }
            }
            Ok(())
        }
        Kind::TarXz => bail!(".tar.xz files are not supported yet"),
        Kind::Bare => bail!("not an archive"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn detects_kinds() {
        assert_eq!(Kind::of("jq-macos-amd64"), Kind::Bare);
        assert_eq!(
            Kind::of("just-1.59.0-x86_64-apple-darwin.tar.gz"),
            Kind::TarGz
        );
        assert_eq!(Kind::of("x.tgz"), Kind::TarGz);
        assert_eq!(Kind::of("helix-25.07.1-x86_64-macos.tar.xz"), Kind::TarXz);
        assert_eq!(Kind::of("x.zip"), Kind::Zip);
    }

    fn tar_gz(path: &Path, entries: &[(&str, &[u8], u32)]) {
        let enc = flate2::write::GzEncoder::new(File::create(path).unwrap(), Default::default());
        let mut tar = tar::Builder::new(enc);
        for (name, data, mode) in entries {
            let mut h = tar::Header::new_gnu();
            h.set_size(data.len() as u64);
            h.set_mode(*mode);
            h.set_cksum();
            tar.append_data(&mut h, name, *data).unwrap();
        }
        tar.into_inner().unwrap().finish().unwrap();
    }

    #[test]
    fn unpacks_tar_gz_with_subfolders_and_modes() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("a.tar.gz");
        tar_gz(
            &archive,
            &[
                ("pkg/tool", b"#!/bin/sh\n", 0o755),
                ("pkg/README", b"hi", 0o644),
            ],
        );
        let dest = dir.path().join("out");
        std::fs::create_dir(&dest).unwrap();
        extract(&archive, Kind::TarGz, &dest).unwrap();

        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(dest.join("pkg/tool"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o755);
        assert_eq!(std::fs::read(dest.join("pkg/README")).unwrap(), b"hi");
    }

    #[test]
    fn unpacks_zip() {
        let dir = tempfile::tempdir().unwrap();
        let archive = dir.path().join("a.zip");
        {
            let mut zip = zip::ZipWriter::new(File::create(&archive).unwrap());
            let opts = zip::write::SimpleFileOptions::default().unix_permissions(0o755);
            zip.start_file("pkg/tool", opts).unwrap();
            zip.write_all(b"#!/bin/sh\n").unwrap();
            zip.finish().unwrap();
        }
        let dest = dir.path().join("out");
        std::fs::create_dir(&dest).unwrap();
        extract(&archive, Kind::Zip, &dest).unwrap();

        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(dest.join("pkg/tool"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o755);
    }
}
