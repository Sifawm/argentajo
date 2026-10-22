//! Job files: a Job with its Layout and output options ↔ one self-contained JSON document, holding
//! its own copy of every Piece, so later changes to the Library don't reach it.

use crate::nest::{Job, Layout};
use crate::output::OutputOptions;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedJob {
    pub job: Job,
    /// `None` when it was saved before nesting, or changed after.
    pub layout: Option<Layout>,
    pub output: OutputOptions,
}

/// The Job file's contents.
pub fn save_job(saved: &SavedJob) -> String {
    serde_json::to_string_pretty(saved).unwrap()
}

/// Refuses anything that isn't a Job file, and a Layout that doesn't fit its Job, so rendering
/// what opens can't fail.
pub fn open_job(json: &[u8]) -> anyhow::Result<SavedJob> {
    let saved: SavedJob = serde_json::from_slice(json)?;
    if let Some(layout) = &saved.layout {
        anyhow::ensure!(layout.fits(&saved.job), "the layout does not match the job");
    }
    Ok(saved)
}

/// A Piece with its cleaned PNG in base64, which the Library keeps in a file of its own.
pub(crate) mod embedded {
    use crate::piece::Piece;
    use base64::{Engine, engine::general_purpose::STANDARD};
    use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error};

    #[derive(Serialize)]
    struct Out<'a> {
        #[serde(flatten)]
        piece: &'a Piece,
        png: String,
    }

    #[derive(Deserialize)]
    struct In {
        #[serde(flatten)]
        piece: Piece,
        png: String,
    }

    pub fn serialize<S: Serializer>(piece: &Piece, s: S) -> Result<S::Ok, S::Error> {
        let png = STANDARD.encode(&piece.png);
        Out { piece, png }.serialize(s)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Piece, D::Error> {
        let In { mut piece, png } = In::deserialize(d)?;
        piece.png = STANDARD.decode(png).map_err(D::Error::custom)?;
        Ok(piece)
    }
}
