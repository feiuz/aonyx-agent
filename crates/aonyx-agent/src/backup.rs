//! Encrypted memory-palace backup (Phase AAA, "cloud-sync").
//!
//! `backup` zips a project's palace (`<root>/.aonyx/{kg,diary,chunks}.db`)
//! and encrypts it with XChaCha20-Poly1305 under a key derived from a
//! passphrase via Argon2id. `restore` reverses it. The resulting file is
//! portable and safe to drop in any cloud (S3, Dropbox, rsync, git) — only
//! the passphrase can open it.
//!
//! File layout: `MAGIC(8) || salt(16) || nonce(24) || ciphertext`.

use std::path::Path;

use anyhow::{anyhow, bail};
use argon2::Argon2;
use chacha20poly1305::aead::{Aead, KeyInit};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
use rand::RngCore;

const MAGIC: &[u8; 8] = b"AONYXBK1";
const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 24;

/// Derive a 32-byte key from `passphrase` + `salt` with Argon2id.
fn derive_key(passphrase: &str, salt: &[u8]) -> anyhow::Result<[u8; 32]> {
    let mut key = [0u8; 32];
    Argon2::default()
        .hash_password_into(passphrase.as_bytes(), salt, &mut key)
        .map_err(|e| anyhow!("key derivation: {e}"))?;
    Ok(key)
}

/// Zip every top-level file in `dir` into an in-memory archive.
fn zip_dir(dir: &Path) -> anyhow::Result<Vec<u8>> {
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::<u8>::new()));
    let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let mut count = 0usize;
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let bytes = std::fs::read(&path)?;
            zip.start_file(name, opts)
                .map_err(|e| anyhow!("zip: {e}"))?;
            zip.write_all(&bytes)?;
            count += 1;
        }
    }
    if count == 0 {
        bail!("palace directory {} has no files to back up", dir.display());
    }
    let cursor = zip.finish().map_err(|e| anyhow!("zip finish: {e}"))?;
    Ok(cursor.into_inner())
}

/// Unzip an in-memory archive into `dest` (flat; path-traversal safe).
fn unzip_into(zip_bytes: &[u8], dest: &Path) -> anyhow::Result<usize> {
    use std::io::Read;

    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip_bytes))
        .map_err(|e| anyhow!("open archive: {e}"))?;
    let mut written = 0usize;
    for i in 0..archive.len() {
        let mut f = archive
            .by_index(i)
            .map_err(|e| anyhow!("archive entry {i}: {e}"))?;
        let name = f.name().to_string();
        // Only ever write the bare file name — never honour `../` paths.
        let safe = Path::new(&name)
            .file_name()
            .ok_or_else(|| anyhow!("invalid archive entry '{name}'"))?;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf)?;
        std::fs::write(dest.join(safe), &buf)?;
        written += 1;
    }
    Ok(written)
}

/// Back up `palace_dir` to `out`, encrypted under `passphrase`.
pub fn backup(palace_dir: &Path, out: &Path, passphrase: &str) -> anyhow::Result<()> {
    if !palace_dir.is_dir() {
        bail!("no palace found at {}", palace_dir.display());
    }
    let plaintext = zip_dir(palace_dir)?;

    let mut salt = [0u8; SALT_LEN];
    let mut nonce = [0u8; NONCE_LEN];
    rand::rngs::OsRng.fill_bytes(&mut salt);
    rand::rngs::OsRng.fill_bytes(&mut nonce);

    let key = derive_key(passphrase, &salt)?;
    let cipher = XChaCha20Poly1305::new(Key::from_slice(&key));
    let ciphertext = cipher
        .encrypt(XNonce::from_slice(&nonce), plaintext.as_ref())
        .map_err(|_| anyhow!("encryption failed"))?;

    let mut buf = Vec::with_capacity(MAGIC.len() + SALT_LEN + NONCE_LEN + ciphertext.len());
    buf.extend_from_slice(MAGIC);
    buf.extend_from_slice(&salt);
    buf.extend_from_slice(&nonce);
    buf.extend_from_slice(&ciphertext);
    std::fs::write(out, &buf)?;
    Ok(())
}

/// Restore an encrypted backup `file` into `palace_dir`.
pub fn restore(
    file: &Path,
    palace_dir: &Path,
    passphrase: &str,
    force: bool,
) -> anyhow::Result<()> {
    let data = std::fs::read(file)?;
    let header = MAGIC.len() + SALT_LEN + NONCE_LEN;
    if data.len() < header || &data[..MAGIC.len()] != MAGIC {
        bail!("{} is not an Aonyx backup file", file.display());
    }
    let salt = &data[MAGIC.len()..MAGIC.len() + SALT_LEN];
    let nonce = &data[MAGIC.len() + SALT_LEN..header];
    let ciphertext = &data[header..];

    let key = derive_key(passphrase, salt)?;
    let cipher = XChaCha20Poly1305::new(Key::from_slice(&key));
    let plaintext = cipher
        .decrypt(XNonce::from_slice(nonce), ciphertext)
        .map_err(|_| anyhow!("decryption failed — wrong passphrase or corrupt file"))?;

    if palace_dir.is_dir() && std::fs::read_dir(palace_dir)?.next().is_some() && !force {
        bail!(
            "palace already exists at {} — pass --force to overwrite",
            palace_dir.display()
        );
    }
    std::fs::create_dir_all(palace_dir)?;
    let n = unzip_into(&plaintext, palace_dir)?;
    eprintln!("aonyx: restored {n} file(s) into {}", palace_dir.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backup_then_restore_round_trips() {
        let src = tempfile::tempdir().unwrap();
        let palace = src.path().join(".aonyx");
        std::fs::create_dir_all(&palace).unwrap();
        std::fs::write(palace.join("kg.db"), b"entities").unwrap();
        std::fs::write(palace.join("diary.db"), b"narrative").unwrap();

        let out = src.path().join("backup.aonyxbak");
        backup(&palace, &out, "correct horse battery staple").unwrap();

        // File starts with the magic header.
        let raw = std::fs::read(&out).unwrap();
        assert_eq!(&raw[..8], MAGIC);

        let dest_root = tempfile::tempdir().unwrap();
        let dest = dest_root.path().join(".aonyx");
        restore(&out, &dest, "correct horse battery staple", false).unwrap();
        assert_eq!(std::fs::read(dest.join("kg.db")).unwrap(), b"entities");
        assert_eq!(std::fs::read(dest.join("diary.db")).unwrap(), b"narrative");
    }

    #[test]
    fn wrong_passphrase_fails() {
        let src = tempfile::tempdir().unwrap();
        let palace = src.path().join(".aonyx");
        std::fs::create_dir_all(&palace).unwrap();
        std::fs::write(palace.join("kg.db"), b"secret").unwrap();
        let out = src.path().join("b.aonyxbak");
        backup(&palace, &out, "right").unwrap();

        let dest = tempfile::tempdir().unwrap().path().join(".aonyx");
        assert!(restore(&out, &dest, "wrong", false).is_err());
    }

    #[test]
    fn rejects_non_backup_file() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("notes.txt");
        std::fs::write(&f, b"hello world this is not a backup").unwrap();
        let dest = dir.path().join(".aonyx");
        assert!(restore(&f, &dest, "x", false).is_err());
    }
}
