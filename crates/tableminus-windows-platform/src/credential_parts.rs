//! Multi-part chunking for secrets exceeding `CRED_MAX_CREDENTIAL_BLOB_SIZE` (2,560 bytes).

use secrecy::zeroize::Zeroizing;
use uuid::Uuid;

/// Backend abstraction for reading, writing, and deleting individual credential parts.
pub(crate) trait PartStore {
    type Error;

    fn read(&self, target: &str) -> Result<Option<Stored>, Self::Error>;

    fn write(
        &self,
        target: &str,
        blob: &[u8],
        label: &str,
        layout: Option<Layout>,
    ) -> Result<(), Self::Error>;

    /// Returns `Ok(false)` when `target` was already absent so cleanup can stop early.
    fn delete(&self, target: &str) -> Result<bool, Self::Error>;
}

/// A single credential part's raw bytes and optional multi-part layout header.
pub(crate) struct Stored {
    pub(crate) blob: Zeroizing<Vec<u8>>,
    pub(crate) layout: Option<Layout>,
}

/// Layout metadata stored in the `TableMinus_Parts` credential attribute on every part.
///
/// Sharing the same `write_id` across all parts ensures a torn write reads as `None`
/// rather than mixing bytes from two different writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Layout {
    pub(crate) write_id: [u8; 16],
    pub(crate) parts: u32,
    pub(crate) bytes: u32,
}

impl Layout {
    pub(crate) const SIZE: usize = 24;

    pub(crate) fn to_bytes(self) -> [u8; Self::SIZE] {
        let mut bytes = [0; Self::SIZE];
        bytes[..16].copy_from_slice(&self.write_id);
        bytes[16..20].copy_from_slice(&self.parts.to_le_bytes());
        bytes[20..].copy_from_slice(&self.bytes.to_le_bytes());
        bytes
    }

    pub(crate) fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != Self::SIZE {
            return None;
        }
        Some(Self {
            write_id: bytes[..16].try_into().ok()?,
            parts: u32::from_le_bytes(bytes[16..20].try_into().ok()?),
            bytes: u32::from_le_bytes(bytes[20..].try_into().ok()?),
        })
    }
}

/// Returns the target name for part `index` (`target` for 0, `target/{index}` for `index > 0`).
pub(crate) fn part_target(target: &str, index: usize) -> String {
    if index == 0 {
        target.to_owned()
    } else {
        format!("{target}/{index}")
    }
}

/// Reads and reassembles a potentially chunked secret from `store`.
pub(crate) fn read_secret<S: PartStore>(
    store: &S,
    target: &str,
) -> Result<Option<Zeroizing<Vec<u8>>>, S::Error> {
    let Some(first) = store.read(target)? else {
        return Ok(None);
    };
    let Some(layout) = first.layout else {
        return Ok(Some(first.blob));
    };

    let mut parts = vec![first.blob];
    for index in 1..layout.parts as usize {
        match store.read(&part_target(target, index))? {
            Some(part) if part.layout == Some(layout) => parts.push(part.blob),
            _ => return Ok(torn(target)),
        }
    }

    let total_size: usize = parts.iter().map(|part| part.len()).sum();
    if total_size != layout.bytes as usize {
        return Ok(torn(target));
    }

    // Sized up front so reallocation never leaves secret bytes in freed heap memory.
    let mut blob = Zeroizing::new(Vec::with_capacity(total_size));
    for part in &parts {
        blob.extend_from_slice(part);
    }
    Ok(Some(blob))
}

fn torn(target: &str) -> Option<Zeroizing<Vec<u8>>> {
    tracing::warn!(
        credential = target,
        "a secret split over several credentials has a part missing or from another write"
    );
    None
}

/// Writes `blob` to `store`, splitting into `part_size` chunks when needed.
pub(crate) fn write_secret<S: PartStore>(
    store: &S,
    target: &str,
    blob: &[u8],
    label: &str,
    part_size: usize,
) -> Result<(), S::Error> {
    let before = held_parts(store, target)?;
    let parts: Vec<&[u8]> = if blob.len() <= part_size {
        vec![blob]
    } else {
        blob.chunks(part_size).collect()
    };

    let layout = match (u32::try_from(parts.len()), u32::try_from(blob.len())) {
        (Ok(count), Ok(bytes)) if count > 1 => Some(Layout {
            write_id: Uuid::new_v4().into_bytes(),
            parts: count,
            bytes,
        }),
        _ => None,
    };

    // Write continuation parts first; part 0 commits the new layout last.
    for (index, part) in parts.iter().enumerate().skip(1) {
        store.write(&part_target(target, index), part, label, layout)?;
    }
    store.write(target, parts[0], label, layout)?;
    delete_parts(store, target, parts.len(), before)
}

/// Deletes a secret and any continuation parts from `store`.
pub(crate) fn delete_secret<S: PartStore>(store: &S, target: &str) -> Result<(), S::Error> {
    let before = held_parts(store, target)?;
    delete_parts(store, target, 1, before)?;
    store.delete(target)?;
    Ok(())
}

fn held_parts<S: PartStore>(store: &S, target: &str) -> Result<usize, S::Error> {
    Ok(store
        .read(target)?
        .and_then(|stored| stored.layout)
        .map_or(1, |layout| layout.parts as usize))
}

fn delete_parts<S: PartStore>(
    store: &S,
    target: &str,
    from: usize,
    before: usize,
) -> Result<(), S::Error> {
    for index in (from..before).rev() {
        store.delete(&part_target(target, index))?;
    }
    let mut index = from.max(before);
    while store.delete(&part_target(target, index))? {
        index += 1;
    }
    Ok(())
}

/// Truncates `label` on a UTF-8 boundary so its UTF-16 encoding fits within `units` code units.
pub(crate) fn fitted(label: &str, units: usize) -> &str {
    let mut used = 0;
    for (index, character) in label.char_indices() {
        used += character.len_utf16();
        if used > units {
            return &label[..index];
        }
    }
    label
}
