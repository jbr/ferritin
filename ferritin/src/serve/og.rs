//! The page card: an on-demand `og:image` for every app route.
//!
//! Every app page's meta tags name an image at this module's [`PREFIX`] plus the
//! page's own path (`/~og/trillium::Handler`), and this handler draws it: a
//! 1200×630 card in the site's light palette — the item path and crate
//! description in JetBrains Mono, the ferritin mark as a 15% watermark bleeding
//! off the right edge, and the wordmark bottom-left. The `~` prefix is the same
//! reservation the pages use: a crate name can never start with `~`, so this
//! namespace cannot shadow one.
//!
//! ## What this is allowed to cost
//!
//! One rasterization, of resident data only. The card's text comes from
//! [`app_page::content`] — the crate-names artifact, never a crate load — so the
//! *content* obeys the same "no fetch reachable by scanners" bound as the meta
//! tags. Drawing it is real CPU, though (parsing the SVG and rasterizing ~10ms
//! of tiny-skia work), which earns two mitigations the tags don't need: the
//! render runs on the big-stack rayon pool rather than the async executor, and
//! the route sits behind its own [`RateLimiter`](super::limiter) bucket. A
//! conditional request never renders at all — the etag is derived from the
//! card's text before any drawing.
//!
//! ## Fonts
//!
//! The card is measured, not shaped: JetBrains Mono is fixed-width (every glyph
//! advances [`CHAR_ADVANCE_EM`]), so line-wrapping is character arithmetic done
//! here, and resvg only has to draw what was already laid out. Both weights are
//! embedded from `assets/og/` (SIL OFL 1.1; license alongside), so rendering
//! never consults system fonts and is identical on every host.

use super::{
    app_page::{self, PageContent, SITE_DESCRIPTION, SITE_NAME, escape},
    caching,
};
use crate::crate_search::CrateSearchService;
use anyhow::Context;
use ferritin_common::CratePath;
use percent_encoding::percent_decode_str;
use rayon::ThreadPool;
use resvg::{
    tiny_skia::{Pixmap, Transform},
    usvg::{Options, Tree, fontdb},
};
use std::sync::{Arc, LazyLock};
use trillium::{Conn, KnownHeaderName, Status};
use trillium_caching_headers::CachingHeadersExt;
use trillium_router::RouterConnExt;

/// The route prefix. Card URLs are the page path under this prefix, so the
/// wildcard tail *is* the app path the card describes.
pub(super) const PREFIX: &str = "/~og";

/// Card dimensions: the 1.91:1 that every unfurl surface crops least.
pub(super) const WIDTH: u32 = 1200;
pub(super) const HEIGHT: u32 = 630;

/// The light ("cold iron, one spark") palette, converted from the oklch values
/// in `client/src/styles/theme.css` — the card should read as a crop of the
/// site.
const BG: &str = "#ecf5fd"; // --bg
const INK: &str = "#1d100e"; // --ink
const MUTED: &str = "#555f69"; // --muted
const ACCENT: &str = "#be4a1b"; // --accent

/// JetBrains Mono's advance width, both weights: 600/1000 units. This constant
/// is what makes layout arithmetic instead of font shaping.
const CHAR_ADVANCE_EM: f32 = 0.6;

/// Horizontal margin; text may occupy `WIDTH - 2 * MARGIN`.
const MARGIN: f32 = 80.0;

/// Title sizes to try, largest first: the biggest whose `::`-boundary wrap fits
/// [`TITLE_MAX_LINES`] wins, and the smallest is used (ellipsized) when none do.
const TITLE_SIZES: &[f32] = &[58.0, 44.0, 34.0];
const TITLE_MAX_LINES: usize = 2;

const DESCRIPTION_SIZE: f32 = 30.0;
const DESCRIPTION_LINE_HEIGHT: f32 = 44.0;
const DESCRIPTION_MAX_LINES: usize = 3;

/// The item path within the crate, set below the description as its own block —
/// the crate is the *context*, the path is the *subject*.
const PATH_SIZE: f32 = 34.0;
const PATH_LINE_HEIGHT: f32 = 46.0;
const PATH_MAX_LINES: usize = 2;

/// Space above the item-path block: enough that the description reads as
/// belonging to the crate name above it, not to the path below.
const PATH_GAP: f32 = 56.0;

/// Watermark opacity, calibrated against *resvg's* rendering (the only one a
/// crawler ever sees) rather than a browser's. Note the opacity must sit on a
/// `<g>` wrapper: on the nested `<svg>` itself it is SVG2-only, and resvg
/// renders it much fainter than specified.
const WATERMARK_OPACITY: f32 = 0.2;

/// The ferritin mark, copied from `client/src/assets/ferritin-mark.svg` (the
/// client only ships its *built* assets under hashed names, so the source file
/// is not reachable from a published crate).
const MARK: &str = include_str!("../../assets/og/ferritin-mark.svg");

static REGULAR: &[u8] = include_bytes!("../../assets/og/JetBrainsMono-Regular.ttf");
static SEMIBOLD: &[u8] = include_bytes!("../../assets/og/JetBrainsMono-SemiBold.ttf");

/// The parse options every card shares: the embedded fonts, and nothing from
/// the host system.
static OPTIONS: LazyLock<Options<'static>> = LazyLock::new(|| {
    let mut fonts = fontdb::Database::new();
    fonts.load_font_data(REGULAR.to_vec());
    fonts.load_font_data(SEMIBOLD.to_vec());
    Options {
        fontdb: Arc::new(fonts),
        ..Options::default()
    }
});

/// One card's text and status, owned so it can cross into the render worker.
///
/// Its `Hash` is the card's cache identity: the drawing is a pure function of
/// this struct and the binary, so hashing it (plus [`caching`]'s build id)
/// names the PNG exactly.
#[derive(Debug, Clone, Hash)]
struct Card {
    /// The crate name (or the site's, or "Not Found"): the largest text.
    title: String,
    /// Set directly under the title, because it describes the *title* — the
    /// crate — and not the item path below it.
    description: String,
    /// The path within the crate (`runtime::Builder::worker_threads`), when the
    /// page names one. Its own block, so a crate-level description is never
    /// read as describing the item.
    item_path: Option<String>,
    status: Status,
}

/// Split one requested path into its crate segment's spelling-independent parts:
/// the rest after the crate segment, or `None` for a crate landing page.
fn path_rest(item_path: &str) -> Option<String> {
    item_path
        .split_once("::")
        .map(|(_crate_segment, rest)| rest.to_string())
}

impl From<PageContent> for Card {
    fn from(content: PageContent) -> Self {
        match content {
            PageContent::Site => Self {
                title: SITE_NAME.to_string(),
                description: SITE_DESCRIPTION.to_string(),
                item_path: None,
                status: Status::Ok,
            },

            PageContent::Known {
                item_path,
                crate_name,
                description,
            } => Self {
                item_path: path_rest(&item_path),
                title: crate_name,
                description,
                status: Status::Ok,
            },

            // The card still renders — a shared link to a typo'd crate unfurls
            // as an honest "Not Found" — but the status stays a 404, for the
            // same reasons the page's does.
            PageContent::NotFound => Self {
                title: "Not Found".to_string(),
                description: SITE_DESCRIPTION.to_string(),
                item_path: None,
                status: Status::NotFound,
            },

            // No artifact to spell the crate's name; the path's own crate
            // segment (minus any `@` requirement) stands in for it.
            PageContent::Indeterminate { item_path } => {
                let CratePath { name, .. } = CratePath::parse(&item_path);
                Self {
                    title: name.to_string(),
                    description: SITE_DESCRIPTION.to_string(),
                    item_path: path_rest(&item_path),
                    status: Status::Ok,
                }
            }
        }
    }
}

impl Card {
    /// The card as an SVG document, laid out and escaped.
    fn svg(&self) -> String {
        let columns =
            |size: f32| ((WIDTH as f32 - 2.0 * MARGIN) / (CHAR_ADVANCE_EM * size)) as usize;

        // The largest title size whose wrap fits; the smallest, ellipsized,
        // when none does.
        let (title_size, title_lines) = TITLE_SIZES
            .iter()
            .find_map(|&size| {
                let lines = wrap_path(&self.title, columns(size));
                (lines.len() <= TITLE_MAX_LINES).then_some((size, lines))
            })
            .unwrap_or_else(|| {
                let size = *TITLE_SIZES.last().expect("TITLE_SIZES is non-empty");
                let mut lines = wrap_path(&self.title, columns(size));
                lines.truncate(TITLE_MAX_LINES);
                ellipsize(&mut lines, columns(size));
                (size, lines)
            });

        let description_lines = wrap_words(
            &self.description,
            columns(DESCRIPTION_SIZE),
            DESCRIPTION_MAX_LINES,
        );

        let path_lines = self.item_path.as_deref().map(|path| {
            let mut lines = wrap_path(path, columns(PATH_SIZE));
            if lines.len() > PATH_MAX_LINES {
                lines.truncate(PATH_MAX_LINES);
                ellipsize(&mut lines, columns(PATH_SIZE));
            }
            lines
        });

        // Vertically center the text block in the area above the footer.
        let title_line_height = title_size * 1.25;
        let title_block = title_lines.len() as f32 * title_line_height;
        let description_block = description_lines.len() as f32 * DESCRIPTION_LINE_HEIGHT;
        let description_gap = if description_lines.is_empty() {
            0.0
        } else {
            24.0
        };
        let path_block = path_lines.as_ref().map_or(0.0, |lines| {
            PATH_GAP + lines.len() as f32 * PATH_LINE_HEIGHT
        });
        let block = title_block + description_gap + description_block + path_block;
        let block_top = 60.0 + (380.0 - block).max(0.0) / 2.0;

        let mut text = String::new();
        // `y` is a baseline in SVG; 0.8em approximates the ascent.
        let mut baseline = block_top + title_size * 0.8;
        for line in &title_lines {
            text.push_str(&format!(
                r#"<text x="{MARGIN}" y="{baseline}" font-family="JetBrains Mono" font-size="{title_size}" font-weight="600" fill="{INK}">{}</text>"#,
                escape(line)
            ));
            baseline += title_line_height;
        }

        baseline = block_top + title_block + description_gap + DESCRIPTION_SIZE * 0.8;
        for line in &description_lines {
            text.push_str(&format!(
                r#"<text x="{MARGIN}" y="{baseline}" font-family="JetBrains Mono" font-size="{DESCRIPTION_SIZE}" fill="{MUTED}">{}</text>"#,
                escape(line)
            ));
            baseline += DESCRIPTION_LINE_HEIGHT;
        }

        if let Some(path_lines) = &path_lines {
            baseline = block_top
                + title_block
                + description_gap
                + description_block
                + PATH_GAP
                + PATH_SIZE * 0.8;
            for line in path_lines {
                text.push_str(&format!(
                    r#"<text x="{MARGIN}" y="{baseline}" font-family="JetBrains Mono" font-size="{PATH_SIZE}" font-weight="600" fill="{INK}">{}</text>"#,
                    escape(line)
                ));
                baseline += PATH_LINE_HEIGHT;
            }
        }

        let mark = mark_polygons();
        format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="{WIDTH}" height="{HEIGHT}" viewBox="0 0 {WIDTH} {HEIGHT}">
<rect width="{WIDTH}" height="{HEIGHT}" fill="{BG}"/>
<g opacity="{WATERMARK_OPACITY}"><svg x="620" y="-80" width="800" height="800" viewBox="0 0 100 100">{mark}</svg></g>
{text}
<svg x="{MARGIN}" y="500" width="52" height="52" viewBox="0 0 100 100">{mark}</svg>
<text x="148" y="534" font-family="JetBrains Mono" font-size="30" fill="{ACCENT}">ferritin.rs</text>
</svg>"#
        )
    }

    /// Rasterize to PNG bytes. Pure CPU; run on a pool worker.
    fn render(&self) -> anyhow::Result<Vec<u8>> {
        let tree = Tree::from_str(&self.svg(), &OPTIONS).context("parsing card svg")?;
        let mut pixmap = Pixmap::new(WIDTH, HEIGHT).context("allocating pixmap")?;
        resvg::render(&tree, Transform::default(), &mut pixmap.as_mut());
        pixmap.encode_png().context("encoding png")
    }
}

/// The mark's shapes, without its document wrapper, for nesting into the card.
fn mark_polygons() -> &'static str {
    let start = MARK.find('>').map_or(0, |end_of_tag| end_of_tag + 1);
    let end = MARK.rfind("</svg>").unwrap_or(MARK.len());
    MARK[start..end].trim()
}

/// Wrap an item path at `::` boundaries into lines of at most `columns`
/// characters, breaking so continuation lines *start* with `::`. A single
/// segment longer than a whole line is broken mid-segment rather than
/// overflowing.
fn wrap_path(path: &str, columns: usize) -> Vec<String> {
    let mut lines: Vec<String> = vec![String::new()];
    for (index, segment) in path.split("::").enumerate() {
        let piece = if index == 0 {
            segment.to_string()
        } else {
            format!("::{segment}")
        };

        let line = lines.last_mut().expect("lines is never empty");
        if line.is_empty() || line.chars().count() + piece.chars().count() <= columns {
            line.push_str(&piece);
        } else {
            lines.push(piece);
        }
    }

    lines
        .iter()
        .flat_map(|line| hard_break(line, columns))
        .collect()
}

/// Greedy word wrap into at most `max_lines` lines of `columns` characters,
/// ellipsizing when the text doesn't fit.
fn wrap_words(text: &str, columns: usize, max_lines: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();

    for word in text.split_whitespace() {
        for word in hard_break(word, columns) {
            let width = current.chars().count();
            if current.is_empty() {
                current = word;
            } else if width + 1 + word.chars().count() <= columns {
                current.push(' ');
                current.push_str(&word);
            } else {
                lines.push(std::mem::replace(&mut current, word));
            }
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }

    if lines.len() > max_lines {
        lines.truncate(max_lines);
        ellipsize(&mut lines, columns);
    }
    lines
}

/// Break one unbreakable run into `columns`-character chunks.
fn hard_break(run: &str, columns: usize) -> Vec<String> {
    debug_assert!(columns > 0);
    let mut chunks = Vec::new();
    let mut chunk = String::new();
    for character in run.chars() {
        if chunk.chars().count() == columns {
            chunks.push(std::mem::take(&mut chunk));
        }
        chunk.push(character);
    }
    chunks.push(chunk);
    chunks
}

/// Mark truncation on the last line, keeping it within `columns`.
fn ellipsize(lines: &mut [String], columns: usize) {
    if let Some(last) = lines.last_mut() {
        while last.chars().count() >= columns {
            last.pop();
        }
        last.push('…');
    }
}

/// Serve the card for the app path under [`PREFIX`].
///
/// The conditional-request short-circuit is checked before anything is drawn:
/// the etag is a hash of the card's text and the build, so a crawler (or the
/// browser cache) revalidating costs a lookup and a hash, not a render.
pub(super) async fn handler(conn: Conn) -> Conn {
    let Some(service) = conn.state::<Arc<CrateSearchService>>().cloned() else {
        return conn.with_status(Status::InternalServerError).halt();
    };
    let Some(pool) = conn.shared_state::<Arc<ThreadPool>>().cloned() else {
        return conn.with_status(Status::InternalServerError).halt();
    };

    let path = percent_decode_str(conn.wildcard().unwrap_or_default()).decode_utf8_lossy();
    let card = Card::from(app_page::content(&service, &path).await);

    let validators = caching::og_image(&card);
    if validators.matches(conn.if_none_match().as_ref()) {
        return validators
            .apply(conn)
            .with_status(Status::NotModified)
            .halt();
    }

    let status = card.status;
    let png = super::run_blocking(&pool, move || card.render()).await;

    match png {
        Some(Ok(png)) => validators
            .apply(conn)
            .with_status(status)
            .with_response_header(KnownHeaderName::ContentType, "image/png")
            .with_body(png)
            .halt(),

        Some(Err(error)) => {
            log::error!("og card render failed: {error:?}");
            conn.with_status(Status::InternalServerError).halt()
        }

        None => conn.with_status(Status::InternalServerError).halt(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(title: &str, description: &str, item_path: Option<&str>) -> Card {
        Card {
            title: title.to_string(),
            description: description.to_string(),
            item_path: item_path.map(str::to_string),
            status: Status::Ok,
        }
    }

    #[test]
    fn wraps_paths_at_segment_boundaries() {
        assert_eq!(wrap_path("serde::Deserialize", 29), ["serde::Deserialize"]);
        assert_eq!(
            wrap_path("tokio::runtime::Builder::worker_threads", 29),
            ["tokio::runtime::Builder", "::worker_threads"]
        );
    }

    #[test]
    fn hard_breaks_oversized_segments() {
        let lines = wrap_path(&"x".repeat(70), 29);
        assert_eq!(lines.len(), 3);
        assert!(lines.iter().all(|line| line.chars().count() <= 29));
    }

    #[test]
    fn wraps_and_ellipsizes_descriptions() {
        let lines = wrap_words("a b c d", 3, 2);
        assert_eq!(lines, ["a b", "c d"]);

        let lines = wrap_words(&"word ".repeat(50), 20, 3);
        assert_eq!(lines.len(), 3);
        assert!(lines.last().unwrap().ends_with('…'));
        assert!(lines.iter().all(|line| line.chars().count() <= 20));
    }

    /// The full pipeline: layout, parse, rasterize, encode. Asserts against the
    /// PNG header's dimensions, which is enough to prove the SVG parsed and the
    /// fonts loaded without snapshotting pixels.
    #[test]
    fn renders_a_png_of_the_declared_size() {
        let png = card(
            "trillium",
            "Build http apps out of composable handlers.",
            Some("Handler"),
        )
        .render()
        .unwrap();

        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(u32::from_be_bytes(png[16..20].try_into().unwrap()), WIDTH);
        assert_eq!(u32::from_be_bytes(png[20..24].try_into().unwrap()), HEIGHT);
    }

    /// Text from crates.io is untrusted; the card must escape it rather than
    /// let it become markup.
    #[test]
    fn escapes_untrusted_text_into_valid_svg() {
        card(
            "evil<crate>",
            r#"a "description" & <script>"#,
            Some(r#"<use href="x"/>"#),
        )
        .render()
        .unwrap();
    }

    #[test]
    fn splits_the_crate_segment_off_the_path() {
        assert_eq!(path_rest("tokio"), None);
        assert_eq!(
            path_rest("tokio@1::runtime::Builder").as_deref(),
            Some("runtime::Builder")
        );
    }
}
