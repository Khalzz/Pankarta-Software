//! The gizmos' icons - `Icon`, and `IconLibrary` (`app.gizmos.icons`).
//!
//! Every icon is an SVG: the engine's own (`Icon::Fire`, `Icon::Mass`...,
//! built into the engine from rendering/gizmos/icons/), or one the game
//! loads by name (`app.gizmos.icons.load("wing", "icons/wing.svg")`, then
//! `Icon::named("wing")`). Each is drawn the first time it's used: into
//! REF_SIZE x REF_SIZE pixels, turned into a distance field (see
//! sdf_text's `signed_distance_field`) and put in the icon atlas - so it's
//! sharp at any size on screen, and takes any color.
//!
//! An icon is one color: what counts is where the SVG covers, not its
//! colors. Anything with a fill or a stroke works - your own drawings,
//! or a downloaded set (Lucide's SVGs, say: 24 x 24, drawn with strokes).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use resvg::{tiny_skia, usvg};

use crate::rendering::sdf_text::signed_distance_field;

/// The size an icon is drawn at before it's turned into a field (px).
pub(crate) const REF_SIZE: u32 = 64;
/// How far out from an icon's edge its field reaches (texels) - also the
/// empty margin round it. Enough for the dark outline round an icon down to
/// 12 px on screen.
const SPREAD: u32 = 8;
/// One icon's field, margin included - one cell of the atlas.
pub(crate) const FIELD_SIZE: u32 = REF_SIZE + SPREAD * 2;
/// The atlas texture's side (texels) - single channel.
pub(crate) const ATLAS_SIZE: u32 = 1024;
/// How many cells fit across the atlas.
const CELLS_ACROSS: u32 = ATLAS_SIZE / FIELD_SIZE;

/// An icon to draw - one of the engine's, or one loaded by name (see
/// `IconLibrary::load`).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Icon {
    // Markers
    Dot,
    Ring,
    Cross,
    Target,
    Diamond,
    Star,
    Pin,
    Flag,
    // Things
    Fire,
    Ball,
    Bolt,
    Drop,
    Wind,
    Gear,
    // Status
    Warning,
    Info,
    /// The checkered circle - a center of mass.
    Mass,
    /// One the game loaded (see `IconLibrary::load`).
    Named(Arc<str>),
}

impl Icon {
    /// The icon loaded as `name` (see `IconLibrary::load`).
    pub fn named(name: &str) -> Self {
        Icon::Named(Arc::from(name))
    }

    /// Every one of the engine's own icons.
    pub const BUILT_IN: [Icon; 17] = [
        Icon::Dot, Icon::Ring, Icon::Cross, Icon::Target, Icon::Diamond, Icon::Star, Icon::Pin, Icon::Flag,
        Icon::Fire, Icon::Ball, Icon::Bolt, Icon::Drop, Icon::Wind, Icon::Gear,
        Icon::Warning, Icon::Info, Icon::Mass,
    ];

    /// Its name - the engine's own in lower case ("fire"), or the name it
    /// was loaded as.
    pub fn name(&self) -> &str {
        match self {
            Icon::Dot => "dot",
            Icon::Ring => "ring",
            Icon::Cross => "cross",
            Icon::Target => "target",
            Icon::Diamond => "diamond",
            Icon::Star => "star",
            Icon::Pin => "pin",
            Icon::Flag => "flag",
            Icon::Fire => "fire",
            Icon::Ball => "ball",
            Icon::Bolt => "bolt",
            Icon::Drop => "drop",
            Icon::Wind => "wind",
            Icon::Gear => "gear",
            Icon::Warning => "warning",
            Icon::Info => "info",
            Icon::Mass => "mass",
            Icon::Named(name) => name,
        }
    }

    /// The engine's SVG for it - None for a loaded one.
    fn built_in_svg(&self) -> Option<&'static str> {
        Some(match self {
            Icon::Dot => include_str!("icons/dot.svg"),
            Icon::Ring => include_str!("icons/ring.svg"),
            Icon::Cross => include_str!("icons/cross.svg"),
            Icon::Target => include_str!("icons/target.svg"),
            Icon::Diamond => include_str!("icons/diamond.svg"),
            Icon::Star => include_str!("icons/star.svg"),
            Icon::Pin => include_str!("icons/pin.svg"),
            Icon::Flag => include_str!("icons/flag.svg"),
            Icon::Fire => include_str!("icons/fire.svg"),
            Icon::Ball => include_str!("icons/ball.svg"),
            Icon::Bolt => include_str!("icons/bolt.svg"),
            Icon::Drop => include_str!("icons/drop.svg"),
            Icon::Wind => include_str!("icons/wind.svg"),
            Icon::Gear => include_str!("icons/gear.svg"),
            Icon::Warning => include_str!("icons/warning.svg"),
            Icon::Info => include_str!("icons/info.svg"),
            Icon::Mass => include_str!("icons/mass.svg"),
            Icon::Named(_) => return None,
        })
    }
}

/// The icons the gizmos can draw - `app.gizmos.icons`. The engine's are
/// always here; the game adds its own with `load`.
#[derive(Default)]
pub struct IconLibrary {
    /// The game's icons, by name - parsed when loaded.
    loaded: HashMap<String, Arc<usvg::Tree>>,
    /// Where each icon drawn so far is in the atlas (u0, v0, u1, v1) -
    /// None: it couldn't be (said once).
    placed: HashMap<Icon, Option<[f32; 4]>>,
    /// The next free cell.
    next_cell: u32,
    /// Fields made since the last frame, for the renderer to copy into the
    /// atlas: where (x, y), and the texels (FIELD_SIZE x FIELD_SIZE).
    pub(crate) uploads: Vec<(u32, u32, Vec<u8>)>,
    /// Names asked for that weren't loaded - said once each.
    missing: HashSet<String>,
}

impl IconLibrary {
    /// Loads the SVG file `path` (inside the assets folder) as the icon
    /// `name` - drawn with `Icon::named(name)`. Loading a name again
    /// replaces it.
    pub fn load(&mut self, name: &str, path: &str) -> Result<(), String> {
        let file = crate::asset_manager::asset_path(path);
        let data = std::fs::read(&file).map_err(|error| format!("icon '{name}': can't read {}: {error}", file.display()))?;
        self.load_svg(name, &data).map_err(|error| format!("icon '{name}' ({path}): {error}"))
    }

    /// Loads `svg` (an SVG document's text or bytes) as the icon `name` - an
    /// icon built into the game (`include_str!`) rather than read from a
    /// file. Loading a name again replaces it.
    pub fn load_svg(&mut self, name: &str, svg: impl AsRef<[u8]>) -> Result<(), String> {
        let tree = usvg::Tree::from_data(svg.as_ref(), &usvg::Options::default()).map_err(|error| format!("not a valid SVG: {error}"))?;
        self.loaded.insert(name.to_owned(), Arc::new(tree));
        // Drawn again the next time it's used.
        self.placed.remove(&Icon::named(name));
        self.missing.remove(name);
        Ok(())
    }

    /// Whether `icon` can be drawn - one of the engine's, or a loaded one.
    pub fn contains(&self, icon: &Icon) -> bool {
        match icon {
            Icon::Named(name) => self.loaded.contains_key(&**name),
            _ => true,
        }
    }

    /// The names of the icons the game loaded.
    pub fn loaded(&self) -> impl Iterator<Item = &str> {
        self.loaded.keys().map(String::as_str)
    }

    /// Where `icon` is in the atlas (u0, v0, u1, v1) - drawn into it the
    /// first time. None when it can't be drawn (said once).
    pub(crate) fn uv(&mut self, icon: &Icon) -> Option<[f32; 4]> {
        if let Some(placed) = self.placed.get(icon) {
            return *placed;
        }
        let tree = match (icon.built_in_svg(), icon) {
            (Some(svg), _) => match usvg::Tree::from_str(svg, &usvg::Options::default()) {
                Ok(tree) => Arc::new(tree),
                Err(error) => {
                    eprintln!("gizmos: the built-in icon '{}' isn't a valid SVG: {error}", icon.name());
                    self.placed.insert(icon.clone(), None);
                    return None;
                }
            },
            (None, Icon::Named(name)) => match self.loaded.get(&**name) {
                Some(tree) => tree.clone(),
                // Not loaded (yet) - not remembered, so it shows once it is.
                None => {
                    if self.missing.insert(name.to_string()) {
                        eprintln!("gizmos: no icon named '{name}' - load it first (app.gizmos.icons.load)");
                    }
                    return None;
                }
            },
            (None, _) => unreachable!("only a named icon has no SVG of the engine's"),
        };
        let placed = self.place(&tree, icon.name());
        self.placed.insert(icon.clone(), placed);
        placed
    }

    /// Draws `tree` into the next free cell of the atlas.
    fn place(&mut self, tree: &usvg::Tree, name: &str) -> Option<[f32; 4]> {
        if self.next_cell >= CELLS_ACROSS * CELLS_ACROSS {
            eprintln!("gizmos: the icon atlas is full ({} icons) - '{name}' can't be drawn", CELLS_ACROSS * CELLS_ACROSS);
            return None;
        }
        let coverage = rasterize(tree)?;
        let pad = SPREAD as usize;
        let field = signed_distance_field(&coverage, REF_SIZE as usize, REF_SIZE as usize, pad, SPREAD as f32);
        let cell = self.next_cell;
        self.next_cell += 1;
        let (x, y) = ((cell % CELLS_ACROSS) * FIELD_SIZE, (cell / CELLS_ACROSS) * FIELD_SIZE);
        self.uploads.push((x, y, field));
        let atlas = ATLAS_SIZE as f32;
        Some([x as f32 / atlas, y as f32 / atlas, (x + FIELD_SIZE) as f32 / atlas, (y + FIELD_SIZE) as f32 / atlas])
    }
}

/// How much of each pixel `tree` covers, drawn into REF_SIZE x REF_SIZE
/// (0..255) - fitted in, centered, whatever its own size.
fn rasterize(tree: &usvg::Tree) -> Option<Vec<u8>> {
    let size = tree.size();
    let scale = REF_SIZE as f32 / size.width().max(size.height());
    let offset_x = (REF_SIZE as f32 - size.width() * scale) / 2.0;
    let offset_y = (REF_SIZE as f32 - size.height() * scale) / 2.0;
    let mut pixmap = tiny_skia::Pixmap::new(REF_SIZE, REF_SIZE)?;
    resvg::render(tree, tiny_skia::Transform::from_row(scale, 0.0, 0.0, scale, offset_x, offset_y), &mut pixmap.as_mut());
    Some(pixmap.pixels().iter().map(|pixel| pixel.alpha()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_built_in_icon_draws() {
        let mut library = IconLibrary::default();
        for icon in Icon::BUILT_IN {
            assert!(library.uv(&icon).is_some(), "{} didn't draw", icon.name());
        }
        assert_eq!(library.uploads.len(), Icon::BUILT_IN.len());
        // Drawn once - asked again, it's where it was.
        let first = library.uv(&Icon::Fire);
        assert_eq!(library.uv(&Icon::Fire), first);
        assert_eq!(library.uploads.len(), Icon::BUILT_IN.len());
    }

    #[test]
    fn an_icon_covers_its_middle_and_not_its_corners() {
        let tree = usvg::Tree::from_str(Icon::Dot.built_in_svg().unwrap(), &usvg::Options::default()).unwrap();
        let coverage = rasterize(&tree).unwrap();
        let at = |x: u32, y: u32| coverage[(y * REF_SIZE + x) as usize];
        assert_eq!(at(REF_SIZE / 2, REF_SIZE / 2), 255);
        assert_eq!(at(0, 0), 0);
    }

    #[test]
    fn a_loaded_icon_is_drawn_by_name() {
        let mut library = IconLibrary::default();
        assert_eq!(library.uv(&Icon::named("wing")), None, "not loaded yet");
        library.load_svg("wing", r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 4"><rect width="10" height="4"/></svg>"#).unwrap();
        assert!(library.contains(&Icon::named("wing")));
        assert!(library.uv(&Icon::named("wing")).is_some());
        assert!(library.load_svg("broken", "not an svg").is_err());
    }
}
