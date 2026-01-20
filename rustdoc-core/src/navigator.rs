use crate::doc_ref::DocRef;
use crate::project::{RustdocData, RustdocProject};
use crate::string_utils::case_aware_jaro_winkler;
use elsa::FrozenMap;
use fieldwork::Fieldwork;
use rustdoc_types::{Id, Item, ItemEnum};
use std::fmt;
use std::fmt::Debug;
use std::fmt::Formatter;
use std::rc::Rc;

/// Represents a single request with its own cache and state
pub struct Navigator {
    pub project: Rc<RustdocProject>,

    // Request-scoped cache
    crate_cache: FrozenMap<String, Box<RustdocData>>,
}

impl Debug for Navigator {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("Navigator")
            .field("project", &self.project)
            .field("crate_cache (len)", &self.crate_cache.len())
            .finish()
    }
}

impl Navigator {
    /// Create a new navigator for a project
    pub fn new(project: Rc<RustdocProject>) -> Self {
        Self {
            crate_cache: FrozenMap::new(),
            project,
        }
    }

    /// Resolve path segments within a specific crate
    pub fn resolve_path<'a>(
        &'a self,
        path: &str,
        suggestions: &mut Vec<Suggestion<'a>>,
    ) -> Option<DocRef<'a, Item>> {
        let (crate_name, index) = if let Some(index) = path.find("::") {
            (&path[..index], Some(index + 2))
        } else {
            (path, None)
        };

        let Some(crate_data) = self.load(crate_name) else {
            suggestions.extend(self.project.available_crates().map(|name| Suggestion {
                path: name.to_string(),
                item: None,
                score: case_aware_jaro_winkler(&name, crate_name),
            }));
            return None;
        };

        // Start from crate root
        let item = crate_data.get(self, &crate_data.root)?;
        if let Some(index) = index {
            self.find_children_recursive(item, path, index, suggestions)
        } else {
            Some(item)
        }
    }

    fn find_children_recursive<'a>(
        &'a self,
        item: DocRef<'a, Item>,
        path: &str,
        index: usize,
        suggestions: &mut Vec<Suggestion<'a>>,
    ) -> Option<DocRef<'a, Item>> {
        let remaining = &path[path.len().min(index)..];
        if remaining.is_empty() {
            return Some(item);
        }
        let segment_end = remaining
            .find("::")
            .map(|x| index + x)
            .unwrap_or(path.len());
        let segment = &path[index..segment_end];
        let next_segment_start = path.len().min(segment_end + 2);

        log::trace!(
            "🔎 searching for {segment} in {} ({:?}) (remaining {})",
            &path[..index],
            item.kind(),
            &path[next_segment_start..]
        );

        for child in item.child_items() {
            if let Some(name) = child.name()
                && name == segment
                && let Some(child) =
                    self.find_children_recursive(child, path, next_segment_start, suggestions)
            {
                return Some(child);
            }
        }

        suggestions.extend(self.generate_suggestions(item, path, index));
        None
    }

    fn generate_suggestions<'a>(
        &'a self,
        item: DocRef<'a, Item>,
        path: &str,
        index: usize,
    ) -> impl Iterator<Item = Suggestion<'a>> {
        item.child_items().filter_map(move |item| {
            item.name().and_then(|name| {
                let full_path = format!("{}{name}", &path[..index]);
                if path.starts_with(&full_path) {
                    None
                } else {
                    let score = case_aware_jaro_winkler(path, &full_path);
                    Some(Suggestion {
                        path: full_path,
                        score,
                        item: Some(item),
                    })
                }
            })
        })
    }

    fn load(&self, crate_name: &str) -> Option<&RustdocData> {
        let crate_name = self.project.normalize_crate_name(crate_name)?;
        match self.crate_cache.get(&*crate_name) {
            Some(docs) => Some(docs),
            None => {
                let crate_data = self.project.load_crate(crate_name)?;
                Some(
                    self.crate_cache
                        .insert(crate_name.to_string(), Box::new(crate_data)),
                )
            }
        }
    }

    pub fn get_item_from_id_path<'a>(
        &'a self,
        crate_name: &str,
        ids: &[u32],
    ) -> Option<(DocRef<'a, Item>, Vec<&'a str>)> {
        let mut path = vec![];
        let crate_docs = self.load(crate_name)?;
        let mut item = crate_docs.get(self, &crate_docs.root)?;
        path.push(item.crate_docs().name());
        for id in ids {
            item = item.get(&Id(*id))?;
            if let ItemEnum::Use(use_item) = item.inner() {
                item = use_item
                    .id
                    .and_then(|id| item.get(&id))
                    .or_else(|| item.navigator().resolve_path(&use_item.source, &mut vec![]))?;
                if !use_item.is_glob {
                    item.set_name(&use_item.name);
                }
            } else if let Some(name) = item.name() {
                path.push(name);
            }
        }

        Some((item, path))
    }
}

// Automatic cleanup when request ends
impl Drop for Navigator {
    fn drop(&mut self) {
        // Cache automatically cleared when Request is dropped
        log::trace!(
            "Navigator dropped, cleaned up {} crates",
            self.crate_cache.len(),
        );
    }
}

#[derive(Fieldwork)]
#[fieldwork(get)]
pub struct Suggestion<'a> {
    path: String,
    item: Option<DocRef<'a, Item>>,
    score: f64,
}
