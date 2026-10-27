//! Printed output: a Layout → each page as an SVG in mm, and the PDF made from those same SVGs, so
//! the preview and the print can't diverge.

use crate::nest::{Job, Layout};
use base64::{Engine, engine::general_purpose::STANDARD};
use pdf_writer::{Content, Finish, Name, Pdf, Rect, Ref};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::Write;
use svg2pdf::usvg;

/// What goes on the page besides the pieces.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OutputOptions {
    /// Each Sheet's outline, to know where to glue it.
    pub sheet_outline: bool,
    /// A line along each Part's contour, as a guide for the saw.
    pub silhouette: bool,
}

impl Default for OutputOptions {
    fn default() -> Self {
        Self {
            sheet_outline: true,
            silhouette: false,
        }
    }
}

/// Page `page` as an SVG in mm: its Sheets with each Part placed on them as nested.
pub fn render_page(job: &Job, layout: &Layout, page: usize, options: &OutputOptions) -> String {
    let [page_w, page_h] = job.paper.size_mm;
    let mut svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{page_w}mm" height="{page_h}mm" viewBox="0 0 {page_w} {page_h}"><defs>"#
    );
    for (i, item) in job.items.iter().enumerate() {
        let [w, h] = item.piece.size_mm;
        _ = write!(
            svg,
            r#"<image id="piece{i}" width="{w}" height="{h}" preserveAspectRatio="none" href="data:image/png;base64,{}"/>"#,
            STANDARD.encode(&item.piece.png)
        );
        // Each Part is placed on its own: it shows only its share of the image.
        // ponytail: the contour runs ~0.5 px inside the motif's edge, so that rim is clipped off.
        if item.piece.contours.len() > 1 {
            for (part, contour) in item.piece.contours.iter().enumerate() {
                _ = write!(
                    svg,
                    r#"<clipPath id="part{i}-{part}"><polygon points="{}"/></clipPath>"#,
                    points(contour)
                );
            }
        }
    }
    _ = write!(
        svg,
        r#"</defs><rect width="{page_w}" height="{page_h}" fill="white"/>"#
    );
    for (sheet, [sheet_w, sheet_h]) in layout.sheets.iter().zip(&job.sheets_mm) {
        if sheet.page != page {
            continue;
        }
        let [x, y] = sheet.origin_mm;
        _ = write!(svg, r#"<g transform="translate({x} {y})">"#);
        if options.sheet_outline {
            _ = write!(
                svg,
                r#"<rect class="sheet" width="{sheet_w}" height="{sheet_h}" fill="none" stroke="black" stroke-width="0.2"/>"#
            );
        }
        for p in &sheet.placements {
            let [a, b, c, d, e, f] = p.transform;
            let contours = &job.items[p.item].piece.contours;
            let clip = if contours.len() > 1 {
                format!(r#" clip-path="url(#part{}-{})""#, p.item, p.part)
            } else {
                String::new()
            };
            // In a group, so the clip path is in the Piece's coordinates too.
            _ = write!(
                svg,
                r##"<g transform="matrix({a} {b} {c} {d} {e} {f})"><use href="#piece{}"{clip}/>"##,
                p.item
            );
            if options.silhouette {
                _ = write!(
                    svg,
                    r#"<polygon class="silhouette" points="{}" fill="none" stroke="black" stroke-width="0.1"/>"#,
                    points(&contours[p.part])
                );
            }
            svg += "</g>";
        }
        svg += "</g>";
    }
    svg + "</svg>"
}

/// The Layout as a PDF, one page per paper page, each the paper's size and drawn from
/// `render_page`'s SVG.
// ponytail: every page embeds every Piece's image, even those not on it; reference them from one
// shared XObject per Piece if multi-page PDFs get too large.
pub fn export_pdf(job: &Job, layout: &Layout, options: &OutputOptions) -> anyhow::Result<Vec<u8>> {
    let [w, h] = job.paper.size_mm.map(|mm| (mm * 72.0 / 25.4) as f32);
    let mut alloc = Ref::new(1);
    let catalog = alloc.bump();
    let page_tree = alloc.bump();
    let mut pdf = Pdf::new();
    let mut pages = vec![];
    for page in 0..layout.pages() {
        let tree = usvg::Tree::from_str(
            &render_page(job, layout, page, options),
            &usvg::Options::default(),
        )?;
        let (chunk, svg) = svg2pdf::to_chunk(&tree, svg2pdf::ConversionOptions::default())
            .map_err(|e| anyhow::anyhow!("could not convert page {}: {e}", page + 1))?;
        let mut renumbered = HashMap::new();
        let chunk = chunk.renumber(|old| *renumbered.entry(old).or_insert_with(|| alloc.bump()));
        let (page_ref, content_ref) = (alloc.bump(), alloc.bump());
        let mut pdf_page = pdf.page(page_ref);
        pdf_page
            .media_box(Rect::new(0.0, 0.0, w, h))
            .parent(page_tree)
            .contents(content_ref);
        pdf_page
            .resources()
            .x_objects()
            .pair(Name(b"Page"), renumbered[&svg]);
        pdf_page.finish();
        // The SVG's XObject is 1 pt square: stretch it over the page.
        let mut content = Content::new();
        content
            .transform([w, 0.0, 0.0, h, 0.0, 0.0])
            .x_object(Name(b"Page"));
        pdf.stream(content_ref, &content.finish());
        pdf.extend(&chunk);
        pages.push(page_ref);
    }
    pdf.catalog(catalog).pages(page_tree);
    pdf.pages(page_tree).count(pages.len() as i32).kids(pages);
    Ok(pdf.finish())
}

fn points(polygon: &[[f64; 2]]) -> String {
    polygon
        .iter()
        .map(|[x, y]| format!("{x},{y}"))
        .collect::<Vec<_>>()
        .join(" ")
}
