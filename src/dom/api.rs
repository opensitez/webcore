//! The WHATWG DOM, as this browser implements it.
//!
//! Method names and shapes are the IDL's — `createElement`, `getElementById`,
//! `cloneNode` — not a spelling of our own. That is the whole point: the same
//! surface is implemented by `vybe_widgets`, and later by a binding to a real
//! browser, so which engine is compiled in is a build-time choice and nothing
//! above this layer knows the difference.
//!
//! Node handles are `u64` here and `u32` in the arena. The public surface takes
//! the wider type because that is what the DOM interface uses everywhere;
//! narrowing happens at the boundary, so the arena is untouched.
//!
//! Every mutation goes through these methods, which update both the arena and the
//! WebCore tree (bridge period), and set dirty flags for incremental re-style/layout.

use crate::css::apply_property;
use crate::dom::arena::NodeId;
use crate::types::{Document, Overflow, Rect, WebCore};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaQueryList {
    pub media: String,
    pub matches: bool,
}

// ─── Read ───────────────────────────────────────────────────────────────────

impl Document {
    /// `element.tagName`.
    pub fn tag_name(&self, id: u32) -> Option<&str> {
        if id == 0 {
            return None;
        }
        if let Some(node) = self.arena.try_get(NodeId(id)) {
            if !node.tag.is_empty() {
                return Some(node.tag.as_str());
            }
        }
        // Shadow nodes have no arena entry — the render tree answers for them.
        self.find_webcore(id).map(|n| n.tag.as_str())
    }

    /// Get an attribute value.
    pub fn get_attribute(&self, id: u32, key: &str) -> Option<String> {
        if id == 0 {
            return None;
        }
        let folded = self.fold_name(key);
        if let Some(v) = self
            .arena
            .try_get(NodeId(id))
            .and_then(|n| n.attributes.get(&folded))
        {
            return Some(v.clone());
        }
        // Shadow-tree nodes are not mirrored into the arena, so the arena has
        // nothing for them and every attribute read came back empty — a
        // `<slot name=title>` reported no name at all. The render tree is the
        // authority for those, and `find_webcore` reaches them.
        self.find_webcore(id)
            .and_then(|n| n.attributes.get(&folded).cloned())
    }

    /// Get the text content of a node and all its descendants.
    pub fn text_content(&self, id: u32) -> String {
        if id == 0 {
            return String::new();
        }
        let mut out = String::new();
        // ⛔ A shadow node is not in the arena — its id comes from a separate
        // descending space — so `collect_text`'s `arena.get` PANICKED on
        // `shadowRoot.querySelector("p").textContent`, an ordinary call with
        // no workaround. The render tree is where a shadow tree lives, and it
        // answers the same question.
        if !self.arena.is_alive(NodeId(id)) {
            if let Some(node) = self.find_webcore(id) {
                Self::collect_text_render(node, &mut out);
            } else if let Some(root) = self.shadow_root_by_id(id) {
                for child in &root.children {
                    Self::collect_text_render(child, &mut out);
                }
            }
            return out;
        }
        self.collect_text(NodeId(id), &mut out);
        out
    }

    /// `textContent` over the RENDER tree, for the nodes the arena has never
    /// held. Same exclusion as [`Self::collect_text`] — comments carry data
    /// and are not content.
    ///
    /// ⛔ That version also excludes processing instructions and this does not,
    /// which is the same rule over a narrower input rather than a looser one:
    /// the only nodes reaching here are shadow trees, built by the HTML parser,
    /// which folds `<?…?>` into a comment (HTML §13.2.5.42). A PI's render box
    /// carries its TARGET as its tag, so a tag-based test could not express the
    /// exclusion anyway. Pinned by
    /// `a_processing_instruction_in_shadow_markup_is_a_comment`.
    fn collect_text_render(node: &WebCore, out: &mut String) {
        if node.tag != "#comment" && !node.text.is_empty() {
            out.push_str(&node.text);
        }
        for child in &node.children {
            Self::collect_text_render(child, out);
        }
    }

    fn collect_text(&self, id: NodeId, out: &mut String) {
        let node = self.arena.get(id);
        // DOM §4.4: `textContent` concatenates the data of every descendant
        // EXCEPT comments and processing instructions. Those carry data too,
        // and including them would leak markup that is not content.
        //
        // Stated as an EXCLUSION rather than "text nodes only" on purpose:
        // `set_text_content` stores the string on the ELEMENT node instead of
        // building a child text node, so an allow-list of `NodeType::Text`
        // silently made `textContent` empty after every such write. Reading the
        // parser alone does not reveal that — it only ever puts text on
        // `#text` nodes.
        let is_markup_only = matches!(
            node.node_type,
            crate::dom::arena::NodeType::Comment
                | crate::dom::arena::NodeType::ProcessingInstruction
        );
        if !is_markup_only && !node.text.is_empty() {
            out.push_str(&node.text);
        }
        let mut child = node.first_child;
        while child.is_some() {
            self.collect_text(child, out);
            child = self.arena.get(child).next_sibling;
        }
    }

    /// `node.parentNode` — 0 when there is none.
    pub fn parent_node(&self, id: u32) -> u32 {
        if id == 0 || crate::dom::events::is_document_target(id) {
            return 0;
        }
        // `<html>`'s parent is the DOCUMENT, not nothing. It read as an orphan,
        // which is why `getRootNode()` stopped at the document element.
        if id == self.root.node_id {
            return crate::dom::events::DOCUMENT_TARGET;
        }
        // The doctype hangs off the document, not off any element.
        if id != 0 && id == self.doctype {
            return crate::dom::events::DOCUMENT_TARGET;
        }
        let p = self.arena.try_get(NodeId(id)).map_or(0, |n| n.parent.0);
        if p == 0 && self.find_webcore(id).is_some() && id != self.root.node_id {
            // A shadow-tree node has no arena parent; its parent is its host.
            if let Some(h) = self.shadow_host_of_child(id) {
                return h;
            }
        }
        p
    }

    /// The node that directly contains `id` in the render tree.
    ///
    /// ⛔ For a TOP-LEVEL shadow child the answer is the shadow ROOT, not the
    /// host. Chrome: `p.parentNode === root` is true and `p.parentElement` is
    /// **null**, because a `ShadowRoot` is a `DocumentFragment` and not an
    /// element. Answering the host here flattened the shadow boundary — the
    /// one boundary the whole feature exists to draw.
    fn shadow_host_of_child(&self, id: u32) -> Option<u32> {
        fn walk(n: &WebCore, id: u32) -> Option<u32> {
            if let Some(sr) = &n.shadow_root {
                if sr.children.iter().any(|c| c.node_id == id) {
                    return Some(sr.node_id);
                }
                for c in &sr.children {
                    if let Some(f) = walk(c, id) {
                        return Some(f);
                    }
                }
            }
            if n.children.iter().any(|c| c.node_id == id) {
                return Some(n.node_id);
            }
            n.children.iter().find_map(|c| walk(c, id))
        }
        walk(&self.root, id)
    }

    /// The HOST element whose shadow tree contains `id`, at any depth.
    ///
    /// `shadow_host_of_child` answers the PARENT, which for a shadow tree's top
    /// level is the shadow root — a different question. Connectedness needs the
    /// host, because that is the node that is or is not in the document.
    ///
    /// ⛔ Walks the whole render tree, and `is_connected` recurses through it
    /// once per nesting level. Fine for the one-or-two shadow roots a page
    /// has; worth knowing before putting `is_connected` in a loop.
    fn shadow_host_of(&self, id: u32) -> Option<u32> {
        fn contains(n: &WebCore, id: u32) -> bool {
            n.node_id == id || n.children.iter().any(|c| contains(c, id))
        }
        fn walk(n: &WebCore, id: u32) -> Option<u32> {
            if let Some(sr) = &n.shadow_root {
                if sr.node_id == id || sr.children.iter().any(|c| contains(c, id)) {
                    return Some(n.node_id);
                }
                for c in &sr.children {
                    if let Some(h) = walk(c, id) {
                        return Some(h);
                    }
                }
            }
            n.children.iter().find_map(|c| walk(c, id))
        }
        walk(&self.root, id)
    }

    /// `document` — the id of the document node itself.
    pub fn document_node(&self) -> u32 {
        crate::dom::events::DOCUMENT_TARGET
    }

    /// `node.childNodes` — every child, of every kind, in tree order.
    pub fn child_nodes(&self, id: u32) -> Vec<u32> {
        if id == 0 {
            return Vec::new();
        }
        // A shadow root's children are its tree's top level; a node INSIDE a
        // shadow tree has children of its own that the arena never saw. Both
        // are answered from the render tree, which is where a shadow tree
        // lives.
        if crate::dom::arena::is_shadow_node_id(id) {
            if let Some(root) = self.shadow_root_by_id(id) {
                return root.children.iter().map(|c| c.node_id).collect();
            }
            return self
                .find_webcore(id)
                .map(|n| n.children.iter().map(|c| c.node_id).collect())
                .unwrap_or_default();
        }
        // ⛔ The DOCUMENT is not an arena node — `arena.children` asserts on
        // its id and PANICKED here, the same unguarded-`get` shape as the
        // shadow ids. Its children are the doctype and the document element,
        // in that order (measured: `document.firstChild === document.doctype`).
        if crate::dom::events::is_document_target(id) {
            let mut kids = Vec::new();
            if self.doctype != 0 {
                kids.push(self.doctype);
            }
            if self.root.node_id != 0 {
                kids.push(self.root.node_id);
            }
            return kids;
        }
        if !self.arena.is_alive(NodeId(id)) {
            return Vec::new();
        }
        self.arena.children(NodeId(id)).map(|c| c.0).collect()
    }

    /// Get the next sibling node_id (0 if none).
    pub fn next_sibling(&self, id: u32) -> u32 {
        if id == 0 {
            return 0;
        }
        self.arena
            .try_get(NodeId(id))
            .map_or(0, |n| n.next_sibling.0)
    }

    /// `node.previousSibling` — 0 when there is none.
    pub fn previous_sibling(&self, id: u32) -> u32 {
        if id == 0 {
            return 0;
        }
        self.arena
            .try_get(NodeId(id))
            .map_or(0, |n| n.prev_sibling.0)
    }
}

// ─── Interactive state reconciliation ───────────────────────────────────────

impl Document {
    /// Copy form state from the render tree back into the arena.
    ///
    /// WHY THIS EXISTS
    ///
    /// Everything in this file dual-writes: the arena first, then the WebCore
    /// tree. Interaction cannot follow that rule. `handle_form_click` and
    /// `process_form_input_key` are free functions over `&mut WebCore` — they
    /// have no `Document`, so they have no arena to write to, and they set
    /// `checked` / `value` on the render tree alone.
    ///
    /// That is invisible until something READS through the arena, which is
    /// exactly what the WHATWG accessors do. Without this, a user ticks a
    /// checkbox and `getAttribute("checked")` still answers the pre-click
    /// value — the interaction happened, and the DOM denies it.
    ///
    /// Reconciling here rather than fixing it at the write sites keeps the
    /// interaction code as pure render-tree logic and puts the sync at the one
    /// boundary that owns both stores.
    ///
    /// Only the three attributes interaction actually writes are copied, and
    /// only on form controls — a click must not cost a clone of every
    /// attribute map in the document.
    /// Not public: this is bookkeeping between two internal stores, not a web
    /// API. Nothing outside the browser should know the render tree and the
    /// arena are separate things.
    pub(crate) fn sync_form_state_to_arena(&mut self) {
        type Snapshot = (u32, Option<String>, Option<String>, Option<String>);
        fn walk(node: &WebCore, out: &mut Vec<Snapshot>) {
            if node.node_id != 0
                && matches!(
                    node.tag.as_str(),
                    "input" | "textarea" | "select" | "option"
                )
            {
                out.push((
                    node.node_id,
                    // `checked` is NOT here any more: interaction writes
                    // CHECKEDNESS on the render-tree box, and the attribute it
                    // used to overwrite is the author's default, which no
                    // click may touch. Syncing it would put the old conflation
                    // back one level down.
                    None,
                    node.attributes.get("value").cloned(),
                    node.attributes.get("selected").cloned(),
                ));
            }
            for child in &node.children {
                walk(child, out);
            }
        }

        let mut updates: Vec<Snapshot> = Vec::new();
        walk(&self.root, &mut updates);

        for (id, checked, value, selected) in updates {
            if !self.arena.is_alive(NodeId(id)) {
                continue;
            }
            let attrs = &mut self.arena.get_mut(NodeId(id)).attributes;
            // Absent is meaningful: `checked` is a boolean attribute, so
            // REMOVING it is how "unticked" is spelled. A set-only sync would
            // make unticking a no-op.
            for (key, val) in [
                ("checked", checked),
                ("value", value),
                ("selected", selected),
            ] {
                match val {
                    Some(v) => {
                        attrs.insert(key.to_string(), v);
                    }
                    None => {
                        attrs.remove(key);
                    }
                }
            }
        }
    }
}

// ─── WHATWG tree operations ─────────────────────────────────────────────────

impl Document {
    /// `element.innerHTML` (getter) — the serialization of a node's CHILDREN,
    /// not of the node itself. `serialize_box` already knew how to write a
    /// subtree; the only thing missing was the DOM spelling in front of it.
    pub fn inner_html(&self, id: u32) -> String {
        if id == 0 {
            return String::new();
        }
        let node = match self.find_webcore(id) {
            Some(n) => n,
            // A node created but never inserted still has an `innerHTML`.
            None => match self.pending_nodes.get(&id) {
                Some(n) => n,
                None => return String::new(),
            },
        };
        let mut out = String::new();
        for child in &node.children {
            crate::html::serializer::serialize_box(child, &mut out);
        }
        out
    }

    /// `node.cloneNode(deep)`. The clone is DETACHED — WHATWG gives it no
    /// parent — so it lands in `pending_nodes` exactly like a freshly created
    /// node, and the same `dom_append_child` attaches it.
    pub fn clone_node(&mut self, id: u32, deep: bool) -> u32 {
        if id == 0 {
            return 0;
        }
        // Cloned out of the tree FIRST so the recursion below owns its source.
        // `clone_subtree` needs `&mut self` for the arena, which it could not
        // have while still borrowing a box out of `self.root`.
        let src = match self.find_webcore(id).cloned() {
            Some(b) => b,
            None => match self.pending_nodes.get(&id).cloned() {
                Some(b) => b,
                None => return 0,
            },
        };
        let clone = self.clone_subtree(&src, deep);
        let new_id = clone.node_id;
        self.pending_nodes.insert(new_id, clone);
        new_id
    }

    /// Recursively rebuild `src` with fresh identities in BOTH stores.
    ///
    /// `src` must be owned by the caller, not borrowed out of `self` — the
    /// arena writes here need `&mut self`.
    fn clone_subtree(&mut self, src: &WebCore, deep: bool) -> WebCore {
        let new_id = match src.tag.as_str() {
            "#text" => self.arena.create_text(&src.text),
            "#comment" => self.arena.create_comment(&src.text),
            _ => {
                let a = self.arena.create_element(&src.tag);
                for (k, v) in &src.attributes {
                    self.arena
                        .get_mut(a)
                        .attributes
                        .insert(k.clone(), v.clone());
                }
                a
            }
        };

        let mut b = src.clone();
        b.node_id = new_id.0;
        b.children.clear();

        // A shallow clone copies the node and no descendants — DOM §4.4.
        if deep {
            for child in &src.children {
                let child_box = self.clone_subtree(child, true);
                self.arena.append_child(new_id, NodeId(child_box.node_id));
                b.children.push(child_box);
            }
        }

        self.next_node_id = self.next_node_id.max(new_id.0 + 1);
        b
    }

    /// `parent.replaceChild(new, old)`.
    ///
    /// Composed from the two operations that already dual-write correctly
    /// rather than open-coded: insert the replacement ahead of the outgoing
    /// node, then remove it. A hand-rolled version would be a third place that
    /// has to remember the arena, the render tree AND the dirty flags.
    pub fn replace_child(&mut self, parent: u32, new_child: u32, old_child: u32) -> bool {
        if parent == 0 || new_child == 0 || old_child == 0 {
            return false;
        }
        if !self.arena.is_alive(NodeId(old_child)) {
            return false;
        }
        // WHATWG throws NotFoundError when `old` is not a child of `parent`.
        if self.arena.get(NodeId(old_child)).parent.0 != parent {
            return false;
        }
        self.insert_before(parent, new_child, old_child);
        self.remove_child(old_child);
        true
    }
}

// ─── Node kind predicates, character data, and the rest of the IDL ──────────

impl Document {
    /// `document.body`.
    pub fn body(&self) -> Option<u32> {
        self.query_selector("body")
    }

    /// Which grammar this document was built from.
    pub fn kind(&self) -> crate::types::DocumentKind {
        self.kind
    }

    /// `node.nodeType === Node.ELEMENT_NODE`.
    pub fn is_element(&self, id: u32) -> bool {
        self.node_type(id) == 1
    }

    pub fn is_text_node(&self, id: u32) -> bool {
        self.node_type(id) == 3
    }

    pub fn is_comment_node(&self, id: u32) -> bool {
        self.node_type(id) == 8
    }

    /// DOM §4.10 `CharacterData` — text, CDATA, comment and processing
    /// instruction. The nodes that HAVE `data`, which is exactly the set whose
    /// `nodeValue` is non-null.
    pub fn is_character_data(&self, id: u32) -> bool {
        matches!(self.node_type(id), 3 | 4 | 7 | 8)
    }

    /// `CharacterData.data` — this node's OWN text, not its descendants'.
    ///
    /// Different question from `text_content`, which concatenates a subtree.
    /// On an element this is empty, because an element has no data of its own.
    pub fn text_data(&self, id: u32) -> String {
        if id == 0 || !self.arena.is_alive(NodeId(id)) {
            return String::new();
        }
        self.arena.get(NodeId(id)).text.clone()
    }

    /// `CharacterData.data = …`.
    pub fn set_text_data(&mut self, id: u32, data: &str) {
        if id == 0 || !self.arena.is_alive(NodeId(id)) {
            return;
        }
        // The `data` SETTER is `replaceData(0, length, data)` (DOM §4.10), and
        // that is exactly how a live range sees it — which is why assigning a
        // whole new string drove a `(1, 4)` range to `(0, 0)` in Chrome rather
        // than leaving the offsets alone.
        let old_len = self.character_data_length(id);
        self.ranges_after_replace_data(id, 0, old_len, data.encode_utf16().count());
        self.set_text_data_raw(id, data);
    }

    /// The write with no range bookkeeping — for callers that have already
    /// done it with better information than "the whole string changed".
    fn set_text_data_raw(&mut self, id: u32, data: &str) {
        if id == 0 || !self.arena.is_alive(NodeId(id)) {
            return;
        }
        self.arena.get_mut(NodeId(id)).text = data.to_string();
        if let Some(node) = self.find_webcore_mut(id) {
            node.text = data.to_string();
            node.layout.layout_dirty = true;
            node.layout.intrinsic_dirty = true;
        }
    }

    /// `CharacterData.length` — in UTF-16 code units, which is what every
    /// offset in this interface is counted in.
    ///
    /// Not `str::len` and not `chars().count()`: DOM offsets are UTF-16, so an
    /// emoji is TWO and `é` is one. A Rust byte offset would disagree with a
    /// script on any non-ASCII text.
    pub fn character_data_length(&self, id: u32) -> usize {
        self.text_data(id).encode_utf16().count()
    }

    /// `CharacterData.substringData(offset, count)`.
    ///
    /// `None` when `offset` is past the end — the spec throws `IndexSizeError`
    /// there, and this is the shape a caller can turn into one. A `count` that
    /// runs past the end is clamped, exactly as the spec says.
    pub fn substring_data(&self, id: u32, offset: usize, count: usize) -> Option<String> {
        let units: Vec<u16> = self.text_data(id).encode_utf16().collect();
        if offset > units.len() {
            return None;
        }
        let end = offset.saturating_add(count).min(units.len());
        Some(String::from_utf16_lossy(&units[offset..end]))
    }

    /// `CharacterData.replaceData(offset, count, data)` — the primitive the
    /// other four mutators are defined in terms of (DOM §4.10).
    pub fn replace_data(&mut self, id: u32, offset: usize, count: usize, data: &str) -> bool {
        let units: Vec<u16> = self.text_data(id).encode_utf16().collect();
        if offset > units.len() {
            return false;
        }
        let end = offset.saturating_add(count).min(units.len());
        let mut out: Vec<u16> = units[..offset].to_vec();
        out.extend(data.encode_utf16());
        out.extend_from_slice(&units[end..]);
        // The PRECISE hook: a boundary past the replaced run slides by the
        // size change and one inside it collapses onto the run's start.
        // Routing through `set_text_data` would report "the whole string
        // changed" and drive every boundary to 0.
        self.ranges_after_replace_data(id, offset, end - offset, data.encode_utf16().count());
        self.set_text_data_raw(id, &String::from_utf16_lossy(&out));
        true
    }

    /// `CharacterData.appendData(data)`.
    pub fn append_data(&mut self, id: u32, data: &str) -> bool {
        let len = self.character_data_length(id);
        self.replace_data(id, len, 0, data)
    }

    /// `CharacterData.insertData(offset, data)`.
    pub fn insert_data(&mut self, id: u32, offset: usize, data: &str) -> bool {
        self.replace_data(id, offset, 0, data)
    }

    /// `CharacterData.deleteData(offset, count)`.
    pub fn delete_data(&mut self, id: u32, offset: usize, count: usize) -> bool {
        self.replace_data(id, offset, count, "")
    }

    /// `Text.splitText(offset)` — cut this node in two and return the NEW node,
    /// which holds everything from `offset` on and becomes the next sibling.
    ///
    /// Returns `None` for a bad offset or a non-text node; the spec throws
    /// `IndexSizeError` for the former.
    pub fn split_text(&mut self, id: u32, offset: usize) -> Option<u32> {
        if self.node_type(id) != 3 {
            return None;
        }
        let tail = self.substring_data(id, offset, usize::MAX)?;
        let new_id = self.create_text_node(&tail);
        // ⛔ A split is not a delete plus an insert as far as a live range is
        // concerned: it has its OWN rule (a boundary past the cut moves to the
        // NEW node), and letting the two internal edits fire their generic
        // hooks would apply a second, wrong adjustment on top of it.
        let was = std::mem::replace(&mut self.suppress_range_updates, true);
        self.delete_data(id, offset, usize::MAX);
        let parent = self.parent_node(id);
        if parent != 0 {
            // Insert directly after this node. A detached text node has no
            // parent and the new node simply stays detached, per the spec.
            let next = self.next_sibling(id);
            if next != 0 {
                self.insert_before(parent, new_id, next);
            } else {
                self.append_child(parent, new_id);
            }
        }
        self.suppress_range_updates = was;
        self.ranges_after_split_text(id, offset, new_id);
        Some(new_id)
    }

    /// `Text.wholeText` — the concatenated data of this node and the run of
    /// text-node siblings it sits in, with no element between them.
    pub fn whole_text(&self, id: u32) -> String {
        if self.node_type(id) != 3 {
            return String::new();
        }
        let parent = self.parent_node(id);
        if parent == 0 {
            return self.text_data(id);
        }
        let sibs = self.child_nodes(parent);
        let Some(at) = sibs.iter().position(|n| *n == id) else {
            return self.text_data(id);
        };
        let mut start = at;
        while start > 0 && self.node_type(sibs[start - 1]) == 3 {
            start -= 1;
        }
        let mut end = at + 1;
        while end < sibs.len() && self.node_type(sibs[end]) == 3 {
            end += 1;
        }
        sibs[start..end]
            .iter()
            .map(|n| self.text_data(*n))
            .collect()
    }

    /// `node.getRootNode()` — the topmost ancestor.
    ///
    /// ⚠ DEVIATION: the spec's answer for a connected node is the DOCUMENT, and
    /// this returns the document ELEMENT (`<html>`), because webcore's tree has
    /// no document node — the only `NodeType::Document` in the arena is the
    /// dead sentinel in slot 0. Everything else about this is spec-correct;
    /// the missing node is tracked as its own item, and it is the same gap that
    /// keeps `ownerDocument` from ever answering null.
    ///
    /// `composed` follows a shadow root out to its host; without it the walk
    /// stops at the shadow boundary, which is the point of a shadow root.
    pub fn get_root_node(&self, id: u32, composed: bool) -> u32 {
        if crate::dom::events::is_document_target(id) {
            return id;
        }
        if id == 0 {
            return 0;
        }
        let mut cur = id;
        loop {
            // A shadow root is the root of its tree unless `composed` asks to
            // cross the boundary — that is the whole point of the option.
            if !composed {
                if let Some(host) = self.shadow_host(cur) {
                    let _ = host;
                    // The shadow tree's root is its topmost node; without a
                    // ShadowRoot node to name, the host stands in for it.
                    if let Some(h) = self.shadow_host(cur) {
                        return h;
                    }
                }
            }
            let parent = self.parent_node(cur);
            if parent == 0 {
                return cur;
            }
            cur = parent;
        }
    }

    /// `node.ownerDocument` — null for the document itself (DOM §4.4), which
    /// is why this is an `Option` rather than the document's own id.
    ///
    /// ⚠ DEVIATION, same cause as `get_root_node`: with no document node in the
    /// tree this hands back the document ELEMENT, and the `None` arm is
    /// unreachable — `node_type` never answers 9 for a live node. Chrome
    /// answers `null` for `document.ownerDocument`; we answer the `<html>`
    /// element for it.
    pub fn owner_document(&self, id: u32) -> Option<u32> {
        if id == 0 {
            return None;
        }
        // Null for the document itself (DOM §4.4) — now reachable, because
        // there is a document node to be.
        if crate::dom::events::is_document_target(id) {
            return None;
        }
        Some(crate::dom::events::DOCUMENT_TARGET)
    }

    /// `node.isSameNode(other)` — identity, not equality.
    pub fn is_same_node(&self, id: u32, other: u32) -> bool {
        id != 0 && id == other
    }

    /// `node.baseURI` — the document base URL.
    pub fn base_uri(&self, _id: u32) -> String {
        self.base_url.clone()
    }

    /// `element.hasAttribute(name)`.
    ///
    /// Not `getAttribute(..).is_some()` at the call site: an attribute present
    /// with an empty value is PRESENT, and `checked=""` is exactly that case.
    pub fn has_attribute(&self, id: u32, name: &str) -> bool {
        if id == 0 || !self.arena.is_alive(NodeId(id)) {
            return false;
        }
        self.arena
            .get(NodeId(id))
            .attributes
            .contains_key(&self.fold_name(name))
    }

    /// `input.checked` — a BOOLEAN ATTRIBUTE: present is true, absent is false.
    /// Its value is irrelevant, which is why this asks `has_attribute`.
    /// `input.checked` — CHECKEDNESS, the live state (HTML §4.10.5.3).
    ///
    /// Not `hasAttribute("checked")`: that is `defaultChecked`, the value a
    /// form reset restores to. They start equal and part company the moment
    /// anything ticks the box.
    pub fn checked(&self, id: u32) -> bool {
        self.find_webcore(id)
            .map(|n| n.checkedness)
            .unwrap_or(false)
    }

    /// `input.checked = b` — the IDL setter.
    ///
    /// Sets checkedness and raises the dirty checkedness flag, which is what
    /// stops a later `setAttribute("checked", …)` moving the box back. It does
    /// NOT touch the attribute: script ticking a box no more rewrites the
    /// document than a user clicking one does.
    ///
    /// This used to add and remove the `checked` ATTRIBUTE, which made the
    /// markup and the state one store — so `getAttribute("checked")` reported
    /// clicks, and the reset algorithm had nothing left to restore to.
    pub fn set_checked(&mut self, id: u32, checked: bool) {
        if let Some(node) = self.find_webcore_mut(id) {
            node.checkedness = checked;
            node.dirty_checked = true;
            // **Invalidate what this changed.** The old body wrote the
            // attribute through `set_attribute`, which marked the node dirty as
            // a side effect; writing the state field directly loses that, and a
            // cached cascade then keeps answering `:checked` from before the
            // change.
            node.layout.layout_dirty = true;
            node.layout.intrinsic_dirty = true;
        }
        // `:checked` is a SELECTOR, so checkedness is an input to the cascade,
        // not just to the painter — and the cascade is cached per element.
        self.style_dirty = true;
    }

    /// Host/browser autofill state for `:autofill`.
    ///
    /// Autofill is not a content attribute, so this updates element state only
    /// and dirties style for selector re-matching.
    pub fn set_autofilled(&mut self, id: u32, autofilled: bool) {
        if let Some(node) = self.find_webcore_mut(id) {
            if node.autofilled == autofilled {
                return;
            }
            node.autofilled = autofilled;
            node.layout.layout_dirty = true;
            node.layout.intrinsic_dirty = true;
        }
        self.style_dirty = true;
    }

    /// `element.focus()`.
    ///
    /// Focus is a field on the document here — `process_key_event` reads
    /// `focused_box` to decide which control receives typing, so setting it IS
    /// focusing.
    pub fn focus(&mut self, id: u32) {
        // ⛔ The first refusal in this crate. An inert subtree cannot take
        // focus, and Chrome's answer when it refuses is that `activeElement`
        // stays exactly where it was — focus does NOT fall back to the nearest
        // focusable ancestor (measured: it remained on the body).
        if self.is_inert(id) {
            return;
        }
        self.focused_box = id;
    }

    /// `getComputedStyle(element)` — the resolved property set after the
    /// cascade.
    ///
    /// Each engine answers with ITS OWN computed-style type (`ComputedStyle`
    /// here, `css::CssProperties` in the other), because the resolved form is
    /// an internal representation and no two engines share one. What crosses
    /// the seam is the string-keyed `computed_style_property` below, which both
    /// answer identically — the same split a browser draws between its internal
    /// style struct and the `CSSStyleDeclaration` it hands to script.
    pub fn get_computed_style(&self, id: u32) -> Option<&crate::types::ComputedStyle> {
        self.find_webcore(id).map(|node| node.style.as_ref())
    }

    /// `font-size` on the root element — what `rem` resolves against.
    pub(crate) fn root_font_px(&self) -> f32 {
        self.root.style.font_size.resolve(16.0, 16.0, 16.0)
    }

    /// The origin a POSITIONED box's insets are measured from: the nearest
    /// positioned ancestor, or the initial containing block at the page origin.
    ///
    /// CSS 2.1 §10.1. Two boxes with the same `top: 10px` sit at different page
    /// coordinates when their containing blocks differ, and `getComputedStyle`
    /// has to answer `10px` for both.
    pub(crate) fn containing_origin(&self, id: u32) -> (f32, f32) {
        let mut current = self.parent_node(id);
        while current != 0 {
            let positioned = self
                .get_computed_style(current)
                .map(|s| !matches!(s.position, crate::types::Position::Static))
                .unwrap_or(false);
            if positioned {
                if let Some(rect) = self.get_bounding_client_rect(current) {
                    return (rect.x, rect.y);
                }
            }
            current = self.parent_node(current);
        }
        (0.0, 0.0)
    }
}

// ─── Node and Element traversal ─────────────────────────────────────────────
//
// All derived from `child_nodes` / `parent_node` / `node_type`, which is the
// point: the DOM offers the same tree through several vocabularies, and a
// browser is expected to answer every one of them. `childNodes` counts text
// and comments; `children` counts only elements — a page that walks the wrong
// one silently sees whitespace as a node.

impl Document {
    /// `node.parentElement` — the parent, but only if it IS an element.
    /// `None` at the document, whose parent is not an element.
    pub fn parent_element(&self, id: u32) -> Option<u32> {
        let parent = self.parent_node(id);
        (parent != 0 && self.is_element(parent)).then_some(parent)
    }

    /// `node.firstChild` / `lastChild` — of ANY kind.
    pub fn first_child(&self, id: u32) -> Option<u32> {
        self.child_nodes(id).first().copied()
    }

    pub fn last_child(&self, id: u32) -> Option<u32> {
        self.child_nodes(id).last().copied()
    }

    pub fn has_child_nodes(&self, id: u32) -> bool {
        !self.child_nodes(id).is_empty()
    }

    /// `node.contains(other)` — DOM §4.4. A node CONTAINS ITSELF, which is the
    /// part that surprises: `a.contains(a)` is true.
    pub fn contains(&self, id: u32, other: u32) -> bool {
        if id == 0 || other == 0 {
            return false;
        }
        let mut current = other;
        loop {
            if current == id {
                return true;
            }
            let parent = self.parent_node(current);
            if parent == 0 {
                return false;
            }
            current = parent;
        }
    }

    /// `element.children` — ELEMENT children only, unlike `childNodes`.
    pub fn children(&self, id: u32) -> Vec<u32> {
        self.child_nodes(id)
            .into_iter()
            .filter(|c| self.is_element(*c))
            .collect()
    }

    pub fn first_element_child(&self, id: u32) -> Option<u32> {
        self.children(id).first().copied()
    }

    pub fn last_element_child(&self, id: u32) -> Option<u32> {
        self.children(id).last().copied()
    }

    pub fn child_element_count(&self, id: u32) -> usize {
        self.children(id).len()
    }

    /// `element.nextElementSibling` — skips text and comments, which is the
    /// difference from `nextSibling` and the reason both exist.
    pub fn next_element_sibling(&self, id: u32) -> Option<u32> {
        let parent = self.parent_node(id);
        let siblings = self.children(parent);
        let at = siblings.iter().position(|n| *n == id)?;
        siblings.get(at + 1).copied()
    }

    pub fn previous_element_sibling(&self, id: u32) -> Option<u32> {
        let parent = self.parent_node(id);
        let siblings = self.children(parent);
        let at = siblings.iter().position(|n| *n == id)?;
        if at == 0 {
            None
        } else {
            siblings.get(at - 1).copied()
        }
    }

    // ─── EventTarget / event handler IDL (DOM §2.7, HTML §8.1.7.2) ──────────

    /// `target.addEventListener(type, callback, options)`.
    pub fn add_event_listener(
        &mut self,
        id: u32,
        event_type: &str,
        handler: crate::dom::events::EventHandler,
        options: crate::dom::events::ListenerOptions,
    ) -> u32 {
        self.event_targets
            .add_event_listener_with(id, event_type, handler, options)
    }

    /// `target.removeEventListener(...)`, by the id `add_event_listener` gave.
    pub fn remove_event_listener(&mut self, listener_id: u32) {
        self.event_targets.remove_event_listener(listener_id);
    }

    /// `target.dispatchEvent(event)` — returns false if a handler cancelled it,
    /// which is what the IDL says the boolean means. The event is NOT trusted:
    /// only the user agent creates trusted events (DOM §2.2).
    pub fn dispatch_event(&mut self, event: &mut crate::dom::events::DomEvent) -> bool {
        event.is_trusted = false;
        self.svg_trigger_event(event.target, event.event_type.as_str());
        // `dispatch_on_tree` only READS the tree, but `event_targets` is a
        // field of the same struct — so the path is collected first and the
        // dispatch runs against it, keeping the borrows apart.
        // Routed through `dispatch_dom_event` so this path and the engine's own
        // get the SAME algorithm — shadow-aware propagation, retargeting, the
        // dispatch flag and the `once` sweep. A second path walker here is how
        // shadow dispatch came to work in one of them and not the other.
        self.dispatch_dom_event(event);
        !event.default_prevented()
    }

    /// `el.onclick = handler` and friends. `None` if `handler_name` is not an
    /// event handler attribute a browser has.
    pub fn set_event_handler(
        &mut self,
        id: u32,
        handler_name: &str,
        handler: crate::dom::events::EventHandler,
    ) -> Option<u32> {
        self.event_targets
            .set_event_handler(id, handler_name, handler)
    }

    /// `el.onclick = null`.
    pub fn remove_event_handler(&mut self, id: u32, handler_name: &str) -> bool {
        self.event_targets.remove_event_handler(id, handler_name)
    }

    /// `el.onclick !== null`.
    pub fn has_event_handler(&self, id: u32, handler_name: &str) -> bool {
        self.event_targets.has_event_handler(id, handler_name)
    }

    /// The handler attributes currently set on an element.
    pub fn event_handler_names(&self, id: u32) -> Vec<String> {
        self.event_targets.event_handler_names(id)
    }

    // ─── Shadow DOM (DOM §4.8, HTML §4.13) ──────────────────────────────────

    /// `element.attachShadow({ mode })` — attach a shadow root and return the
    /// HOST's id.
    ///
    /// A `ShadowRoot` is a node in the spec, and this tree has no node for it
    /// (the same shape as the missing Document node), so the host is what comes
    /// back and the shadow tree is reached through `shadow_children`. Returns
    /// `None` if the element already has one — `attachShadow` throws
    /// `NotSupportedError` there rather than replacing it.
    pub fn attach_shadow(&mut self, id: u32, mode: crate::types::ShadowMode) -> Option<u32> {
        if id == 0 {
            return None;
        }
        if self.has_shadow_root(id) {
            return None;
        }
        let node = self.find_webcore_mut(id)?;
        node.attach_shadow(mode, "");
        // Returns the SHADOW ROOT, as the IDL says — not the host.
        node.shadow_root.as_ref().map(|sr| sr.node_id)
    }

    /// Resolve a shadow root id to the element that hosts it.
    fn host_of_shadow_root(&self, shadow_id: u32) -> Option<u32> {
        fn walk(n: &WebCore, sid: u32) -> Option<u32> {
            if let Some(sr) = &n.shadow_root {
                if sr.node_id == sid {
                    return Some(n.node_id);
                }
                for c in &sr.children {
                    if let Some(f) = walk(c, sid) {
                        return Some(f);
                    }
                }
            }
            n.children.iter().find_map(|c| walk(c, sid))
        }
        walk(&self.root, shadow_id)
    }

    /// Borrow a shadow root by its own id.
    fn shadow_root_by_id(&self, shadow_id: u32) -> Option<&crate::types::ShadowRoot> {
        let host = self.host_of_shadow_root(shadow_id)?;
        self.find_webcore(host)?.shadow_root.as_deref()
    }

    fn shadow_root_by_id_mut(&mut self, shadow_id: u32) -> Option<&mut crate::types::ShadowRoot> {
        let host = self.host_of_shadow_root(shadow_id)?;
        self.find_webcore_mut(host)?.shadow_root.as_deref_mut()
    }

    /// `shadowRoot.host`.
    pub fn shadow_root_host(&self, shadow_id: u32) -> Option<u32> {
        self.host_of_shadow_root(shadow_id)
    }

    /// `shadowRoot.mode`.
    pub fn shadow_root_mode(&self, shadow_id: u32) -> Option<crate::types::ShadowMode> {
        self.shadow_root_by_id(shadow_id).map(|sr| sr.mode)
    }

    /// `shadowRoot.delegatesFocus`.
    pub fn shadow_delegates_focus(&self, shadow_id: u32) -> bool {
        self.shadow_root_by_id(shadow_id)
            .map(|sr| sr.delegates_focus)
            .unwrap_or(false)
    }
    pub fn set_shadow_delegates_focus(&mut self, shadow_id: u32, v: bool) {
        if let Some(sr) = self.shadow_root_by_id_mut(shadow_id) {
            sr.delegates_focus = v;
        }
    }

    /// `shadowRoot.slotAssignment`.
    pub fn shadow_slot_assignment(&self, shadow_id: u32) -> crate::types::SlotAssignment {
        self.shadow_root_by_id(shadow_id)
            .map(|sr| sr.slot_assignment)
            .unwrap_or_default()
    }
    pub fn set_shadow_slot_assignment(&mut self, shadow_id: u32, v: crate::types::SlotAssignment) {
        if let Some(sr) = self.shadow_root_by_id_mut(shadow_id) {
            sr.slot_assignment = v;
        }
    }

    /// `shadowRoot.clonable`.
    pub fn shadow_clonable(&self, shadow_id: u32) -> bool {
        self.shadow_root_by_id(shadow_id)
            .map(|sr| sr.clonable)
            .unwrap_or(false)
    }
    pub fn set_shadow_clonable(&mut self, shadow_id: u32, v: bool) {
        if let Some(sr) = self.shadow_root_by_id_mut(shadow_id) {
            sr.clonable = v;
        }
    }

    /// `shadowRoot.serializable`.
    pub fn shadow_serializable(&self, shadow_id: u32) -> bool {
        self.shadow_root_by_id(shadow_id)
            .map(|sr| sr.serializable)
            .unwrap_or(false)
    }
    pub fn set_shadow_serializable(&mut self, shadow_id: u32, v: bool) {
        if let Some(sr) = self.shadow_root_by_id_mut(shadow_id) {
            sr.serializable = v;
        }
    }

    /// `shadowRoot.adoptedStyleSheets` — constructed sheets applied to the
    /// tree, held as their source text.
    pub fn shadow_adopted_stylesheets(&self, shadow_id: u32) -> Vec<String> {
        self.shadow_root_by_id(shadow_id)
            .map(|sr| sr.adopted_stylesheets.clone())
            .unwrap_or_default()
    }

    /// Replace `adoptedStyleSheets` and fold the rules into the scoped sheet,
    /// so adopting one actually STYLES the tree rather than only recording it.
    pub fn set_shadow_adopted_stylesheets(&mut self, shadow_id: u32, sheets: Vec<String>) {
        let Some(sr) = self.shadow_root_by_id_mut(shadow_id) else {
            return;
        };
        sr.adopted_stylesheets = sheets.clone();
        for css in &sheets {
            sr.stylesheet.parse_and_add_author(css);
        }
        sr.stylesheet.rebuild_index();
    }

    /// `shadowRoot.activeElement` — the focused node, when it is inside THIS
    /// shadow tree. Focus outside it is not this root's business.
    pub fn shadow_active_element(&self, shadow_id: u32) -> Option<u32> {
        let focused = self.focused_box;
        if focused == 0 {
            return None;
        }
        let host = self.host_of_shadow_root(shadow_id)?;
        let sr = self.find_webcore(host)?.shadow_root.as_ref()?;
        fn contains(nodes: &[WebCore], id: u32) -> bool {
            nodes
                .iter()
                .any(|n| n.node_id == id || contains(&n.children, id))
        }
        if contains(&sr.children, focused) {
            Some(focused)
        } else {
            None
        }
    }

    /// `element.shadowRoot` — present only for an OPEN root. A closed root is
    /// invisible from outside, which is the whole point of closed mode.
    pub fn shadow_root_of(&self, id: u32) -> Option<u32> {
        let node = self.find_webcore(id)?;
        let sr = node.shadow_root.as_ref()?;
        match sr.mode {
            crate::types::ShadowMode::Open => Some(sr.node_id),
            crate::types::ShadowMode::Closed => None,
        }
    }

    /// Does this element have a shadow root, open or closed?
    pub fn has_shadow_root(&self, id: u32) -> bool {
        self.find_webcore(id)
            .map(|n| n.shadow_root.is_some())
            .unwrap_or(false)
    }

    /// `shadowRoot.host` — the element a shadow tree is attached to.
    pub fn shadow_host(&self, node_id: u32) -> Option<u32> {
        fn walk(n: &WebCore, target: u32) -> Option<u32> {
            if let Some(sr) = &n.shadow_root {
                for c in &sr.children {
                    if contains(c, target) {
                        return Some(n.node_id);
                    }
                }
            }
            for c in &n.children {
                if let Some(h) = walk(c, target) {
                    return Some(h);
                }
            }
            if let Some(sr) = &n.shadow_root {
                for c in &sr.children {
                    if let Some(h) = walk(c, target) {
                        return Some(h);
                    }
                }
            }
            None
        }
        fn contains(n: &WebCore, target: u32) -> bool {
            if n.node_id == target {
                return true;
            }
            if let Some(sr) = &n.shadow_root {
                if sr.children.iter().any(|c| contains(c, target)) {
                    return true;
                }
            }
            n.children.iter().any(|c| contains(c, target))
        }
        walk(&self.root, node_id)
    }

    /// `shadowRoot.children` — the top-level nodes of an element's shadow tree.
    pub fn shadow_children(&self, id: u32) -> Vec<u32> {
        self.find_webcore(id)
            .and_then(|n| n.shadow_root.as_ref())
            .map(|sr| {
                sr.children
                    .iter()
                    .filter(|c| c.is_element())
                    .map(|c| c.node_id)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// `shadowRoot.querySelector(sel)` — scoped to the shadow tree.
    ///
    /// `document.querySelector` deliberately does NOT reach in here; this is
    /// the way in, and without it a shadow tree was unreachable through any
    /// API at all.
    pub fn shadow_query_selector(&self, host: u32, selector: &str) -> Option<u32> {
        self.shadow_query_selector_all(host, selector)
            .first()
            .copied()
    }

    /// `shadowRoot.querySelectorAll(sel)`.
    pub fn shadow_query_selector_all(&self, host: u32, selector: &str) -> Vec<u32> {
        let Some(node) = self.find_webcore(host) else {
            return Vec::new();
        };
        let Some(sr) = node.shadow_root.as_ref() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for child in &sr.children {
            out.extend(crate::dom::query::matching_ids_from(child, selector, false));
        }
        out
    }

    /// `shadowRoot.innerHTML = html` — replace a shadow tree's contents.
    pub fn set_shadow_inner_html(&mut self, host: u32, html: &str) -> bool {
        match self.find_webcore_mut(host) {
            Some(node) => node.set_shadow_content(html),
            None => false,
        }
    }

    // ─── HTMLSlotElement (HTML §4.13.4) ─────────────────────────────────────

    /// `slot.assignedNodes()` — the light-DOM nodes projected into this slot.
    ///
    /// Empty when nothing matches, which is when the slot's own children render
    /// as FALLBACK instead. `flatten` is not modelled separately: nested slots
    /// are projected during `resolve_slots`, so the answer is already flat.
    pub fn assigned_nodes(&self, slot_id: u32) -> Vec<u32> {
        let Some(host) = self.shadow_host(slot_id) else {
            return Vec::new();
        };
        let Some(slot) = self.find_webcore(slot_id) else {
            return Vec::new();
        };
        if slot.tag != "slot" {
            return Vec::new();
        }
        let name = slot.attributes.get("name").cloned().unwrap_or_default();
        let Some(host_node) = self.find_webcore(host) else {
            return Vec::new();
        };
        host_node
            .children
            .iter()
            .filter(|c| {
                let assigned = c.attributes.get("slot").cloned().unwrap_or_default();
                if name.is_empty() {
                    assigned.is_empty()
                        && (c.is_element() || (c.is_text_node() && !c.text.trim().is_empty()))
                } else {
                    assigned == name
                }
            })
            .map(|c| c.node_id)
            .collect()
    }

    /// `slot.assignedElements()` — `assignedNodes` without the text nodes.
    pub fn assigned_elements(&self, slot_id: u32) -> Vec<u32> {
        self.assigned_nodes(slot_id)
            .into_iter()
            .filter(|id| {
                self.find_webcore(*id)
                    .map(|n| n.is_element())
                    .unwrap_or(false)
            })
            .collect()
    }

    /// `element.assignedSlot` — the slot a light-DOM child is projected into,
    /// or `None` when it is not slotted.
    pub fn assigned_slot(&self, id: u32) -> Option<u32> {
        let parent = self.parent_node(id);
        if parent == 0 || !self.has_shadow_root(parent) {
            return None;
        }
        let want = self
            .find_webcore(id)?
            .attributes
            .get("slot")
            .cloned()
            .unwrap_or_default();
        fn find_slot(nodes: &[WebCore], want: &str) -> Option<u32> {
            for n in nodes {
                if n.tag == "slot" {
                    let name = n.attributes.get("name").map(|s| s.as_str()).unwrap_or("");
                    if name == want {
                        return Some(n.node_id);
                    }
                }
                if let Some(f) = find_slot(&n.children, want) {
                    return Some(f);
                }
            }
            None
        }
        let host = self.find_webcore(parent)?;
        find_slot(&host.shadow_root.as_ref()?.children, &want)
    }

    /// Every `<slot>` in an element's shadow tree, in tree order.
    pub fn shadow_slots(&self, host: u32) -> Vec<u32> {
        let Some(node) = self.find_webcore(host) else {
            return Vec::new();
        };
        let Some(sr) = node.shadow_root.as_ref() else {
            return Vec::new();
        };
        fn collect(nodes: &[WebCore], out: &mut Vec<u32>) {
            for n in nodes {
                if n.tag == "slot" {
                    out.push(n.node_id);
                }
                collect(&n.children, out);
            }
        }
        let mut out = Vec::new();
        collect(&sr.children, &mut out);
        out
    }

    /// Fire `slotchange` on every slot of a host whose assignment changed.
    ///
    /// The event does not bubble past the shadow root in the spec's sense, but
    /// it IS a real event a component listens for to react to its light DOM
    /// changing — and nothing fired it before.
    pub fn fire_slot_change(&mut self, host: u32) {
        for slot in self.shadow_slots(host) {
            let mut e = crate::dom::events::DomEvent::new("slotchange", slot);
            self.dispatch_dom_event(&mut e);
        }
    }

    /// `element.matches(selectors)`.
    ///
    /// Answered against the whole document, because a selector can name
    /// ancestors and siblings: `matches("div p")` is a question about where the
    /// element SITS, not about the element alone. It used to run the
    /// simple-selector matcher, which returned false for any selector with a
    /// combinator — and `closest()`, which is built on this, inherited that.
    pub fn matches(&self, id: u32, selectors: &str) -> bool {
        id != 0 && self.query_selector_all(selectors).contains(&id)
    }

    /// `element.closest(selectors)` — the nearest ancestor-or-self that
    /// matches. Starts at the element ITSELF, per DOM §4.9.
    pub fn closest(&self, id: u32, selectors: &str) -> Option<u32> {
        // The match set is resolved ONCE and then walked up, rather than
        // re-querying the document at every ancestor — `matches` is a whole-tree
        // query now, so the naive loop was O(depth × tree).
        let hits: std::collections::HashSet<u32> =
            self.query_selector_all(selectors).into_iter().collect();
        let mut current = id;
        while current != 0 {
            if hits.contains(&current) {
                return Some(current);
            }
            current = self.parent_node(current);
        }
        None
    }

    /// `getElementsByClassName(names)` — every element carrying ALL the named
    /// classes, in tree order. Space-separated, and an empty list matches
    /// nothing.
    pub fn get_elements_by_class_name(&self, names: &str) -> Vec<u32> {
        let wanted: Vec<&str> = names.split_whitespace().collect();
        if wanted.is_empty() {
            return Vec::new();
        }
        self.get_elements_by_tag_name("*")
            .into_iter()
            .filter(|id| wanted.iter().all(|c| self.class_list_contains(*id, c)))
            .collect()
    }

    /// `element.hasAttributes()`.
    pub fn has_attributes(&self, id: u32) -> bool {
        !self.get_attribute_names(id).is_empty()
    }

    /// `element.toggleAttribute(name)` — adds when absent, removes when
    /// present, and answers whether it is present afterwards.
    pub fn toggle_attribute(&mut self, id: u32, name: &str) -> bool {
        if self.has_attribute(id, name) {
            self.remove_attribute(id, name);
            false
        } else {
            self.set_attribute(id, name, "");
            true
        }
    }

    /// `element.className` — the `class` attribute as written.
    pub fn class_name(&self, id: u32) -> String {
        self.get_attribute(id, "class").unwrap_or_default()
    }

    pub fn set_class_name(&mut self, id: u32, value: &str) {
        self.set_attribute(id, "class", value);
    }

    /// `element.id`.
    pub fn id(&self, id: u32) -> String {
        self.get_attribute(id, "id").unwrap_or_default()
    }

    pub fn set_id(&mut self, id: u32, value: &str) {
        self.set_attribute(id, "id", value);
    }

    /// `element.remove()` — ChildNode §4.2.9. Detaches the node from wherever
    /// it is, without the caller naming the parent.
    pub fn remove(&mut self, id: u32) {
        self.remove_child(id);
    }

    /// `document.documentElement` — the root element, `<html>`.
    pub fn document_element(&self) -> Option<u32> {
        Some(self.root.node_id).filter(|id| *id != 0)
    }

    /// `document.head`.
    pub fn head(&self) -> Option<u32> {
        self.query_selector("head")
    }

    /// `document.activeElement` — what has focus.
    pub fn active_element(&self) -> Option<u32> {
        (self.focused_box != 0).then_some(self.focused_box)
    }

    /// `element.blur()`.
    pub fn blur(&mut self, id: u32) {
        if self.focused_box == id {
            self.focused_box = 0;
        }
    }
}

// ─── Viewport ───────────────────────────────────────────────────────────────

impl Document {
    /// The viewport this document is laid out against, as `(width, height)`.
    ///
    /// `window.innerWidth` / `window.innerHeight` read THIS — a window's size
    /// and its document's viewport are one measurement, not two.
    pub fn viewport(&self) -> (f32, f32) {
        (self.viewport_w, self.viewport_h)
    }

    /// `window.resizeTo`, or a real window resize.
    ///
    /// Relayouts immediately: every percentage length, every `vw`/`vh` and the
    /// whole flow depend on this width, so a viewport change that has not been
    /// laid out is a document that disagrees with itself.
    pub fn set_viewport(&mut self, width: f32, height: f32) {
        self.viewport_w = width;
        self.viewport_h = height;
        crate::layout::LayoutEngine::new().layout(self, width);
    }
}

// ─── Name folding ───────────────────────────────────────────────────────────

impl Document {
    /// Fold a tag or attribute name the way THIS document does.
    ///
    /// One function, so the places that take a name — creating an element,
    /// writing an attribute, reading one back, looking up by tag — cannot
    /// disagree about whether this document folds.
    ///
    /// HTML is ASCII-case-insensitive for these names, so `createElement("DIV")`
    /// makes a `div` and `getAttribute("HREF")` finds `href`. XML is
    /// case-sensitive: `<Rect>` and `<rect>` are two different elements, and
    /// folding one into the other would silently merge them.
    pub(crate) fn fold_name(&self, name: &str) -> String {
        match self.kind {
            crate::types::DocumentKind::Html => name.to_ascii_lowercase(),
            crate::types::DocumentKind::Xml => name.to_string(),
        }
    }
}

// ─── WHATWG Node accessors ──────────────────────────────────────────────────
//
// The arena has carried the model for all of these from the start —
// `NodeType`, the attribute map, the parent chain. What it lacked was the IDL
// spelling on top. These are that spelling, and nothing more: no new state, no
// second store to keep in sync.
//
// The numbering is DOM §4.4 `nodeType`, which is also what `vybe_widgets`
// answers — the two engines are only interchangeable if they agree digit for
// digit, so these constants are copied from the spec table rather than
// re-derived.

impl Document {
    /// `node.nodeType` — DOM §4.4. `0` for a node that does not exist, which
    /// is not a spec value: the spec has no way to ask about a missing node.
    pub fn node_type(&self, id: u32) -> u16 {
        // DOCUMENT_NODE. The document is not in the arena — it is a reserved
        // id — so this answers before the liveness check, which would reject it.
        if crate::dom::events::is_document_target(id) {
            return 9;
        }
        // A `ShadowRoot` IS a `DocumentFragment` (DOM §4.8) — but only the
        // ROOT. Every node INSIDE a shadow tree is numbered from the same
        // descending id space, so "is a shadow id" would call a shadow `<p>` a
        // fragment; the question is whether this id names a shadow root.
        if crate::dom::arena::is_shadow_node_id(id) {
            if self.shadow_root_by_id(id).is_some() {
                return 11;
            }
            return match self.find_webcore(id) {
                Some(n) if n.tag == "#text" => 3,
                Some(n) if n.tag == "#comment" => 8,
                Some(_) => 1,
                None => 0,
            };
        }
        if id == 0 || !self.arena.is_alive(NodeId(id)) {
            return 0;
        }
        match self.arena.get(NodeId(id)).node_type {
            crate::dom::arena::NodeType::Element => 1,
            crate::dom::arena::NodeType::Text => 3,
            crate::dom::arena::NodeType::CData => 4,
            crate::dom::arena::NodeType::ProcessingInstruction => 7,
            crate::dom::arena::NodeType::Comment => 8,
            crate::dom::arena::NodeType::Document => 9,
            crate::dom::arena::NodeType::DocumentType => 10,
            crate::dom::arena::NodeType::DocumentFragment => 11,
        }
    }

    /// `node.nodeName` — the tag for an element, `#text` / `#comment` /
    /// `#document` for the rest. The arena already stores exactly those
    /// strings in `tag`, so this is a read, not a mapping.
    ///
    /// NOT upper-cased. WHATWG upper-cases only for HTML elements in an HTML
    /// document, and `vybe_widgets::node_name` returns the tag verbatim; an
    /// engine that upper-cased here would answer differently from the engine
    /// it is meant to replace.
    pub fn node_name(&self, id: u32) -> String {
        if id == 0 {
            return String::new();
        }
        // A `ShadowRoot` IS a `DocumentFragment` (DOM §4.8), and every node
        // answers a `nodeName` — a fragment's is `#document-fragment`. Shadow
        // nodes are not arena nodes, so the arena guard below answered `""`
        // for the whole shadow tree.
        if !self.arena.is_alive(NodeId(id)) {
            if self.shadow_root_by_id(id).is_some() {
                return "#document-fragment".to_string();
            }
            if let Some(node) = self.find_webcore(id) {
                return match node.tag.as_str() {
                    "#text" | "#comment" => node.tag.clone(),
                    tag => tag.to_ascii_uppercase(),
                };
            }
            return String::new();
        }
        let node = self.arena.get(NodeId(id));
        let name = node.tag.clone();
        // **An element's `nodeName` is its HTML-UPPERCASED qualified name**
        // (DOM §4.9): "Let qualifiedName be this's qualified name. If this is
        // in the HTML namespace and its node document is an HTML document,
        // then return qualifiedName in ASCII uppercase. Return qualifiedName."
        //
        // This answered the STORED tag, which HTML folds to lowercase on the
        // way in, so `el.nodeName == "DIV"` — the check a page actually writes
        // — was false for every element. `local_name` is the lowercase half.
        //
        // ⛔ BOTH conditions, not just the document. An `<svg:rect>` inside an
        // HTML page is in the SVG namespace and keeps its case; uppercasing on
        // the document kind alone renamed it to `SVG:RECT`, which the
        // namespace test here caught.
        let html_namespace = match node.namespace.as_deref() {
            None => true,
            Some(ns) => ns == crate::dom::HTML_NAMESPACE,
        };
        if matches!(self.kind, crate::types::DocumentKind::Html)
            && matches!(node.node_type, crate::dom::arena::NodeType::Element)
            && html_namespace
        {
            return name.to_ascii_uppercase();
        }
        name
    }

    /// `node.nodeValue` — the data of a text or comment node. `None` for an
    /// element and for the document, per DOM §4.4.
    pub fn node_value(&self, id: u32) -> Option<String> {
        if id == 0 || !self.arena.is_alive(NodeId(id)) {
            return None;
        }
        let node = self.arena.get(NodeId(id));
        // DOM §4.4: `nodeValue` is non-null for text, CDATA, comment and
        // processing-instruction nodes — everything that carries DATA. It is
        // null for an element and for the document.
        match node.node_type {
            crate::dom::arena::NodeType::Text
            | crate::dom::arena::NodeType::CData
            | crate::dom::arena::NodeType::Comment
            | crate::dom::arena::NodeType::ProcessingInstruction => Some(node.text.clone()),
            _ => None,
        }
    }

    /// `node.isConnected` — whether the node's root is the document.
    ///
    /// Walks to the root rather than asking "does it have a parent": a child
    /// of a DETACHED subtree has a parent and is still not connected. A node
    /// sitting in `pending_nodes` was created but never inserted, so it is
    /// disconnected by construction.
    pub fn is_connected(&self, id: u32) -> bool {
        if id == 0 {
            return false;
        }
        // DOM §4.4: connected means the SHADOW-INCLUDING root is the document.
        // A node in a shadow tree is connected exactly when its host is, and
        // it is not an arena node — so the guard below called every one of
        // them detached.
        if !self.arena.is_alive(NodeId(id)) {
            if let Some(host) = self.shadow_host_of(id) {
                return self.is_connected(host);
            }
            return false;
        }
        if self.pending_nodes.contains_key(&id) {
            return false;
        }
        let root = self.root.node_id;
        if id == root {
            return true;
        }
        let mut cur = NodeId(id);
        while cur.is_some() {
            let parent = self.arena.get(cur).parent;
            if parent.0 == root {
                return true;
            }
            cur = parent;
        }
        false
    }

    /// `element.getAttributeNames()` — in insertion order is NOT guaranteed
    /// here: the arena stores attributes in a `HashMap`, so the order is
    /// arbitrary. WHATWG specifies document order; callers that care must
    /// sort. Named here rather than silently differing.
    pub fn get_attribute_names(&self, id: u32) -> Vec<String> {
        if id == 0 {
            return Vec::new();
        }
        // DOM §4.9: "the qualified names of the attributes in this element's
        // attribute list, in order" — the list order, not a hash order.
        //
        // Through `attribute_entries`, so a SHADOW element answers too: it has
        // no arena entry, and reading the arena directly returned an empty
        // list for every node in a shadow tree.
        self.attribute_entries(id)
            .into_iter()
            .map(|(k, _)| k)
            .collect()
    }

    /// `document.getElementsByTagName()` — tree order, ASCII case-insensitive
    /// as HTML requires (`<DIV>` and `<div>` are the same tag). Returns a
    /// SNAPSHOT, not a live `HTMLCollection`; liveness cannot survive a
    /// `Vec<u32>` return and is lost at this boundary either way.
    pub fn get_elements_by_tag_name(&self, tag: &str) -> Vec<u32> {
        // Folded, not blanket-lowercased: an HTML document matches
        // case-insensitively because BOTH sides fold, while an XML document
        // must distinguish `<Rect>` from `<rect>`. Hardcoding
        // `eq_ignore_ascii_case` here would have made XML lookups match the
        // wrong elements.
        let want = self.fold_name(tag);
        let all = want == "*";
        let mut out = Vec::new();
        fn walk(doc: &Document, node: &WebCore, want: &str, all: bool, out: &mut Vec<u32>) {
            if all || doc.fold_name(&node.tag) == want {
                // Text and comment boxes carry `#text` / `#comment` as their
                // tag and are not elements, so `*` must not collect them.
                if doc.node_type(node.node_id) == 1 {
                    out.push(node.node_id);
                }
            }
            for child in &node.children {
                walk(doc, child, want, all, out);
            }
        }
        walk(self, &self.root, &want, all, &mut out);
        out
    }

    /// `document.title`.
    ///
    /// Read from the document's own field, NOT by finding a `<title>` element:
    /// the parser lifts the title out of `<head>` as it goes and head content
    /// is not kept as a render box, so looking for the element answered `""`
    /// for every parsed document.
    pub fn title(&self) -> String {
        self.title.clone()
    }

    /// `document.title = …`.
    ///
    /// Writes the field, AND the `<title>` element when the document has one —
    /// a tree built through `createElement` keeps its element, and the two must
    /// not disagree about what the title is.
    pub fn set_title(&mut self, title: &str) {
        self.title = title.to_string();
        if let Some(node) = self.query_selector("title") {
            self.set_text_content(node, title);
        }
    }
}

// ─── Mutate ─────────────────────────────────────────────────────────────────

impl Document {
    /// Create a new element node. Returns its stable node_id.
    /// The node is detached — use `dom_append_child` or `dom_insert_before` to attach it.
    pub fn create_element(&mut self, tag: &str) -> u32 {
        // `createElement("DIV")` makes a `div` in an HTML document — the name
        // is folded here so every later lookup by tag agrees with it.
        // `createElementNS` deliberately does NOT fold; see it below.
        let tag = &self.fold_name(tag);
        let arena_id = self.arena.create_element(tag);
        let mut b = WebCore::new(tag);
        b.node_id = arena_id.0;
        apply_property(
            std::sync::Arc::make_mut(&mut b.style),
            "display",
            crate::html::default_display(tag),
        );
        self.pending_nodes.insert(arena_id.0, b);
        self.next_node_id = self.next_node_id.max(arena_id.0 + 1);
        arena_id.0
    }

    /// `document.createTextNode(data)`.
    pub fn create_text_node(&mut self, text: &str) -> u32 {
        let arena_id = self.arena.create_text(text);
        let mut b = WebCore::new("#text");
        b.node_id = arena_id.0;
        b.text = text.to_string();
        self.pending_nodes.insert(arena_id.0, b);
        self.next_node_id = self.next_node_id.max(arena_id.0 + 1);
        arena_id.0
    }

    /// Create a new comment node — `document.createComment()`. Detached, like
    /// its element and text siblings above.
    ///
    /// The render box is `display:none`: a comment is in the tree and in
    /// `childNodes`, and it draws nothing. Without that it would inherit the
    /// default display for an unknown tag and paint its own text onto the page.
    pub fn create_comment(&mut self, text: &str) -> u32 {
        let arena_id = self.arena.create_comment(text);
        let mut b = WebCore::new("#comment");
        b.node_id = arena_id.0;
        b.text = text.to_string();
        apply_property(std::sync::Arc::make_mut(&mut b.style), "display", "none");
        self.pending_nodes.insert(arena_id.0, b);
        self.next_node_id = self.next_node_id.max(arena_id.0 + 1);
        arena_id.0
    }

    /// Append a child node to a parent.
    /// If child is in pending_nodes (just created), it's moved into the tree.
    /// If child is already in the tree, it's detached from its current parent first.
    pub fn append_child(&mut self, parent_id: u32, child_id: u32) {
        if parent_id == 0 || child_id == 0 {
            return;
        }

        // **A fragment inserts its CHILDREN and not itself** (DOM §4.2.1).
        // Appending the fragment node would put a `#document-fragment` box in
        // the tree — something no selector matches and no layout knows — and
        // leave the caller's nodes one level deeper than they asked for.
        if self.is_document_fragment(child_id) {
            for node in self.child_nodes(child_id) {
                self.append_child(parent_id, node);
            }
            return;
        }

        // Where the node is about to land, for the live-range hook below.
        let insert_index = self.child_nodes(parent_id).len();

        // Update arena
        let arena_parent = self
            .arena
            .try_get(NodeId(child_id))
            .map_or(crate::dom::arena::NodeId::NONE, |n| n.parent);
        if arena_parent.is_some() {
            self.arena.remove_child(NodeId(child_id));
        }
        self.arena.append_child(NodeId(parent_id), NodeId(child_id));

        // Update WebCore tree
        let child_box = if let Some(b) = self.pending_nodes.remove(&child_id) {
            b
        } else {
            self.detach_webcore(child_id)
                .unwrap_or_else(|| WebCore::new("#error"))
        };

        if let Some(parent) = self.find_webcore_mut(parent_id) {
            parent.children.push(child_box);
            parent.layout.layout_dirty = true;
            parent.layout.intrinsic_dirty = true;
            parent.has_dirty_layout_descendant = true;
        }
        self.ranges_after_insert(parent_id, insert_index);
    }

    /// Insert a child before a reference node.
    pub fn insert_before(&mut self, parent_id: u32, child_id: u32, reference_id: u32) {
        if parent_id == 0 || child_id == 0 || reference_id == 0 {
            return;
        }

        // A fragment splices its children in — see `append_child`. Each goes
        // before the SAME reference, which is what keeps them in order.
        if self.is_document_fragment(child_id) {
            for node in self.child_nodes(child_id) {
                self.insert_before(parent_id, node, reference_id);
            }
            return;
        }

        let insert_index = self
            .child_nodes(parent_id)
            .iter()
            .position(|c| *c == reference_id)
            .unwrap_or(0);

        // Update arena
        let arena_parent = self
            .arena
            .try_get(NodeId(child_id))
            .map_or(crate::dom::arena::NodeId::NONE, |n| n.parent);
        if arena_parent.is_some() {
            self.arena.remove_child(NodeId(child_id));
        }
        self.arena
            .insert_before(NodeId(parent_id), NodeId(child_id), NodeId(reference_id));

        // Update WebCore tree
        let child_box = if let Some(b) = self.pending_nodes.remove(&child_id) {
            b
        } else {
            self.detach_webcore(child_id)
                .unwrap_or_else(|| WebCore::new("#error"))
        };

        if let Some(parent) = self.find_webcore_mut(parent_id) {
            let idx = parent
                .children
                .iter()
                .position(|c| c.node_id == reference_id)
                .unwrap_or(parent.children.len());
            parent.children.insert(idx, child_box);
            parent.layout.layout_dirty = true;
            parent.layout.intrinsic_dirty = true;
            parent.has_dirty_layout_descendant = true;
        }
        self.ranges_after_insert(parent_id, insert_index);
    }

    /// Remove a child from its parent. The node is dropped from the WebCore tree
    /// and freed in the arena.
    pub fn remove_child(&mut self, child_id: u32) {
        if child_id == 0 {
            return;
        }
        // Get parent before removing
        let parent_id = self
            .arena
            .try_get(NodeId(child_id))
            .map_or(0, |n| n.parent.0);
        // DOM §6.1: every live `NodeIterator` is told BEFORE the node leaves,
        // because the rule needs the tree it is about to lose — the reference
        // moves to what precedes or follows the removed subtree, and neither
        // is reachable once the links are cut.
        self.run_pre_removing_steps(child_id);
        // Live ranges too, and for the same reason: a range whose container is
        // inside the doomed subtree moves to `(parent, index)`, and neither is
        // reachable once the links are cut.
        if parent_id != 0 {
            let index = self
                .child_nodes(parent_id)
                .iter()
                .position(|c| *c == child_id)
                .unwrap_or(0);
            self.ranges_before_remove(child_id, parent_id, index);
        }
        self.arena.remove_child(NodeId(child_id));
        // **A removed node is DETACHED, not destroyed** (DOM §4.2.3):
        // `removeChild` hands the node back and the caller may insert it
        // somewhere else, which is how every "move this node" is written.
        // Freeing the arena slot here made that idiom read a dead node —
        // `remove` then `appendChild` elsewhere lost the subtree silently, and
        // `replaceWith` left the replaced node unusable.
        //
        // It goes where a freshly CREATED node goes, because it is now in the
        // same state: detached, with an id, waiting to be attached. That is
        // the map `append_child` and `insert_before` already look in.
        if let Some(detached) = self.detach_webcore(child_id) {
            self.pending_nodes.insert(child_id, detached);
        }
        // Mark parent dirty for layout
        if parent_id != 0 {
            if let Some(parent) = self.find_webcore_mut(parent_id) {
                parent.layout.layout_dirty = true;
                parent.layout.intrinsic_dirty = true;
                parent.has_dirty_layout_descendant = true;
            }
        }
    }

    /// Set an attribute on an element. Sets STYLE dirty flag + layout dirty.
    pub fn set_attribute(&mut self, id: u32, key: &str, value: &str) {
        if id == 0 {
            return;
        }
        let key = &self.fold_name(key);
        self.arena.set_attribute(NodeId(id), key, value);
        let mut canvas_resized = None;
        if let Some(node) = self.find_webcore_mut(id) {
            node.attributes.insert(key, value);
            node.layout.layout_dirty = true;
            node.layout.intrinsic_dirty = true;
            // "When the `checked` content attribute is added, if the control
            // does not have dirty checkedness, the user agent must set the
            // checkedness of the element to true" (HTML §4.10.5.3). The
            // attribute is `defaultChecked`, so it only reaches the live state
            // while nothing has claimed that state yet.
            if key == "checked" && !node.dirty_checked {
                node.checkedness = true;
            }
            // The same rule for the other two states the markup seeds. A
            // content attribute reaches the live state only while nothing has
            // claimed that state — which is what makes it the DEFAULT.
            if key == "value" && !node.dirty_value {
                node.value_state = None;
                crate::html::forms::seed_input_value(node);
            }
            if key == "selected" && node.tag == "option" && !node.dirty_selectedness {
                node.selectedness = true;
            }
            // §4.12.5: `<canvas>`'s `width`/`height` IDL attributes REFLECT
            // these content attributes, and setting either one reinitialises
            // the bitmap and the drawing state. Reached this way — rather than
            // only through `set_canvas_size` — because `setAttribute` is a
            // route a page has to the same value, and two ways to set one
            // thing must not differ in what they do.
            if node.tag == "canvas" && (key == "width" || key == "height") {
                if let Ok(n) = value.trim().parse::<u32>() {
                    canvas_resized = Some(if key == "width" {
                        (n, node.image_height)
                    } else {
                        (node.image_width, n)
                    });
                }
            }
        }
        if let Some((w, h)) = canvas_resized {
            self.set_canvas_size(id, w, h);
        }
        // "If an option element in the list of options asks for a reset, then
        // run that select element's selectedness setting algorithm" — the
        // second half of the `selected` rule above, which only the PARENT can
        // do: it is what enforces one-selection-at-a-time and what refreshes
        // the label a closed drop-down shows.
        if key == "selected" {
            if let Some(select) = self.parent_select_of(id) {
                self.notify_select_changed(select);
            }
        }
    }

    /// The `<select>` an option belongs to, if any.
    pub(crate) fn parent_select_of(&self, option: u32) -> Option<u32> {
        fn walk(node: &WebCore, target: u32) -> Option<u32> {
            if node.tag == "select" && crate::html::forms::option_ids(node).contains(&target) {
                return Some(node.node_id);
            }
            node.children.iter().find_map(|c| walk(c, target))
        }
        walk(&self.root, option)
    }

    /// Re-settle a `<select>` after its options changed: run the selectedness
    /// setting algorithm, then move the shown label to match.
    ///
    /// ⛔ Needed on every route that adds or removes an option, not just the
    /// parser's. A drop-down built through the DOM had no selection at all,
    /// because only the parse path ever ran the algorithm — so a `ComboBox`
    /// filled by `AddItem` came up blank where the same markup came up on its
    /// first option.
    pub(crate) fn notify_select_changed(&mut self, select: u32) {
        if let Some(sel) = self.find_webcore_mut(select) {
            crate::html::forms::run_selectedness_setting_algorithm(sel);
            crate::html::forms::refresh_select_display_text(sel);
            sel.layout.layout_dirty = true;
        }
        self.style_dirty = true;
    }

    /// Remove an attribute from an element. Sets STYLE dirty flag.
    pub fn remove_attribute(&mut self, id: u32, key: &str) {
        if id == 0 {
            return;
        }
        let key = &self.fold_name(key);
        self.arena.remove_attribute(NodeId(id), key);
        if let Some(node) = self.find_webcore_mut(id) {
            node.attributes.remove(key);
            // The other half of the same sentence: "when the `checked` content
            // attribute is removed, if the control does not have dirty
            // checkedness, the user agent must set the checkedness of the
            // element to false."
            if key == "checked" && !node.dirty_checked {
                node.checkedness = false;
            }
        }
    }

    /// Set the text content of a node, replacing all children.
    pub fn set_text_content(&mut self, id: u32, text: &str) {
        if id == 0 {
            return;
        }

        // **A replaced child is DETACHED, not destroyed** — the rule
        // `remove_child` already states for DOM §4.2.3, and this is the same
        // removal. Freeing the slot here did more than lose a node a script
        // might re-insert: it RECYCLED the id, and everything keyed by node id
        // is then aliased. The listener map is: the next element created took a
        // dead node's id and inherited its handlers, so a control that cleared
        // and rebuilt itself ended up with two listeners on one button, one of
        // them belonging to an element that no longer existed.
        for child in self.child_nodes(id) {
            self.remove_child(child);
        }
        if let Some(node) = self.find_webcore_mut(id) {
            node.text.clear();
        }

        // **The replacement is a TEXT NODE, not a string on the element**
        // (DOM §4.4: "string replace all"). An element's own `text` field is
        // read by layout for a text node and for a pseudo-element and nowhere
        // else — `has_direct_text` in `layout/mod.rs` — so writing it here put
        // the words in the DOM, in `textContent` and in `outerHTML` while
        // laying out and painting NOTHING.
        //
        // That is what emptied the .NET forms: a label's caption, a button's
        // face and a form's title all arrive as `textContent`, so every control
        // rendered as a bare rectangle. Markup that came through the parser was
        // unaffected — it builds real `#text` boxes — which is exactly why a
        // page written in HTML showed its words and a page built through the
        // DOM did not.
        //
        // Empty text is the spec's null case: children are removed and NO node
        // is inserted, which is also what keeps `<p></p>` from gaining a child
        // the markup never had.
        if !text.is_empty() {
            let node = self.create_text_node(text);
            self.append_child(id, node);
            // A node that has just JOINED the tree has no computed style —
            // `append_child` marks layout dirty but nothing re-runs the
            // cascade, and a text run with no inherited font measures to
            // nothing. `:empty` also stops matching the parent, which is a
            // second reason this is a style change and not only a layout one.
            self.style_dirty = true;
        }
    }

    /// Parse HTML and replace the children of the given node.
    pub fn set_inner_html(&mut self, id: u32, html: &str) {
        if id == 0 {
            return;
        }

        // Parse HTML fragment
        let fragment = crate::html::parse_html(html);
        // The fragment comes back wrapped in `<html><head></head><body>…`, and
        // `innerHTML` means the CONTENT. Take the body's children by finding
        // body, not by indexing: this used to check `children[0]`, which the
        // synthesised `<head>` displaced.
        let new_children: Vec<WebCore> =
            match fragment.root.children.iter().position(|c| c.tag == "body") {
                Some(at) => {
                    let mut children = fragment.root.children;
                    std::mem::take(&mut children[at].children)
                }
                None => fragment.root.children,
            };

        // Detached, not destroyed — see `set_text_content`, which this is the
        // markup-shaped twin of. A freed id gets RECYCLED, and every map keyed
        // by node id then aliases a dead node onto a live one.
        for child in self.child_nodes(id) {
            self.remove_child(child);
        }

        // Set new children on WebCore
        if let Some(node) = self.find_webcore_mut(id) {
            node.children = new_children;
            node.text.clear();
        }

        // Rebuild arena for new children — split borrow: &mut self.arena + &mut self.root
        if let Some(node) = crate::dom::find_box_mut(&mut self.root, id) {
            for child in &mut node.children {
                crate::html::rebuild_arena_recursive_pub(&mut self.arena, child, NodeId(id));
            }
        }
    }

    // ── classList ──

    /// Add a class to the element's class list.
    pub fn class_list_add(&mut self, id: u32, class: &str) {
        if id == 0 || class.is_empty() {
            return;
        }
        let current = self.get_attribute(id, "class").unwrap_or_default();
        if current.split_whitespace().any(|c| c == class) {
            return;
        }
        let new_val = if current.is_empty() {
            class.to_string()
        } else {
            format!("{} {}", current, class)
        };
        self.set_attribute(id, "class", &new_val);
    }

    /// Remove a class from the element's class list.
    pub fn class_list_remove(&mut self, id: u32, class: &str) {
        if id == 0 || class.is_empty() {
            return;
        }
        let current = self.get_attribute(id, "class").unwrap_or_default();
        let new_val: Vec<&str> = current.split_whitespace().filter(|&c| c != class).collect();
        let joined = new_val.join(" ");
        if joined.is_empty() {
            self.remove_attribute(id, "class");
        } else {
            self.set_attribute(id, "class", &joined);
        }
    }

    /// Toggle a class. Returns true if the class is now present.
    pub fn class_list_toggle(&mut self, id: u32, class: &str) -> bool {
        if self.class_list_contains(id, class) {
            self.class_list_remove(id, class);
            false
        } else {
            self.class_list_add(id, class);
            true
        }
    }

    /// Check if an element has a class.
    pub fn class_list_contains(&self, id: u32, class: &str) -> bool {
        if id == 0 {
            return false;
        }
        self.get_attribute(id, "class")
            .as_deref()
            .map(|c| c.split_whitespace().any(|cl| cl == class))
            .unwrap_or(false)
    }

    // ── Inline style ──

    /// Set a single CSS property in the element's inline style.
    pub fn set_style_property(&mut self, id: u32, prop: &str, value: &str) {
        if id == 0 {
            return;
        }
        let prop_lower = prop.to_ascii_lowercase();
        if !prop_lower.starts_with("--")
            && crate::css::properties::resolve(&prop_lower)
                == crate::css::properties::PropertyId::Unknown
        {
            return;
        }
        let current = self.get_attribute(id, "style").unwrap_or_default();
        let mut props = parse_inline_style(&current);
        // CSSOM §6.7.2: `setProperty(prop, "")` REMOVES the declaration. It
        // used to store the empty string, which serialized as a malformed
        // `position: ` — a declaration the parser then dropped, so the property
        // looked removed while `style` carried a syntax error that any
        // round-trip through `cssText` would preserve.
        if value.trim().is_empty() {
            props.retain(|decl| decl.name != prop_lower);
            if let Some(longhands) = inline_shorthand_longhands(&prop_lower) {
                props.retain(|decl| !longhands.contains(&decl.name.as_str()));
            }
        } else if let Some(expanded) = expand_inline_shorthand(&prop_lower, value) {
            props.retain(|decl| {
                decl.name != prop_lower
                    && inline_shorthand_longhands(&prop_lower)
                        .map(|longhands| !longhands.contains(&decl.name.as_str()))
                        .unwrap_or(true)
            });
            props.extend(expanded);
        } else if let Some(entry) = props.iter_mut().find(|decl| decl.name == prop_lower) {
            let (new_value, important) = split_important_suffix(value);
            entry.value = new_value;
            entry.important = important;
        } else {
            let (new_value, important) = split_important_suffix(value);
            props.push(InlineStyleDecl {
                name: prop_lower,
                value: new_value,
                important,
            });
        }
        let new_style = serialize_inline_style(&props);
        self.set_attribute(id, "style", &new_style);
    }

    /// Get a single CSS property from the element's inline style.
    pub fn get_style_property(&self, id: u32, prop: &str) -> Option<String> {
        let style_attr = self.get_attribute(id, "style")?;
        let prop_lower = prop.to_ascii_lowercase();
        parse_inline_style(&style_attr)
            .into_iter()
            .find(|decl| decl.name == prop_lower)
            .map(|decl| decl.value)
    }

    /// Get a single CSS property's priority from the element's inline style.
    pub fn get_style_property_priority(&self, id: u32, prop: &str) -> Option<String> {
        let style_attr = self.get_attribute(id, "style")?;
        let prop_lower = prop.to_ascii_lowercase();
        parse_inline_style(&style_attr)
            .into_iter()
            .find(|decl| decl.name == prop_lower)
            .map(|decl| {
                if decl.important {
                    "important".to_string()
                } else {
                    String::new()
                }
            })
    }

    /// Count declarations in the element's inline style.
    pub fn style_property_len(&self, id: u32) -> usize {
        self.get_attribute(id, "style")
            .map(|style| parse_inline_style(&style).len())
            .unwrap_or(0)
    }

    /// Return the declaration name at `index` in the element's inline style.
    pub fn style_property_item(&self, id: u32, index: usize) -> Option<String> {
        self.get_attribute(id, "style")
            .and_then(|style| parse_inline_style(&style).into_iter().nth(index))
            .map(|decl| decl.name)
    }

    /// Remove a CSS property from the element's inline style.
    pub fn remove_style_property(&mut self, id: u32, prop: &str) -> Option<String> {
        if id == 0 {
            return None;
        }
        let current = self.get_attribute(id, "style").unwrap_or_default();
        let prop_lower = prop.to_ascii_lowercase();
        let mut removed = None;
        let props: Vec<InlineStyleDecl> = parse_inline_style(&current)
            .into_iter()
            .filter(|decl| {
                if decl.name == prop_lower {
                    if removed.is_none() {
                        removed = Some(decl.value.clone());
                    }
                    false
                } else {
                    true
                }
            })
            .collect();
        if props.is_empty() {
            self.remove_attribute(id, "style");
        } else {
            let new_style = serialize_inline_style(&props);
            self.set_attribute(id, "style", &new_style);
        }
        removed
    }

    // ── Stylesheet CSSOM ──

    /// Number of document stylesheets exposed through the current aggregate sheet.
    pub fn style_sheet_len(&self) -> usize {
        1
    }

    /// Serialized `cssRules` for the aggregate document stylesheet.
    pub fn style_sheet_css_rules(&self, sheet_index: usize) -> Option<Vec<String>> {
        if sheet_index == 0 {
            Some(self.stylesheet.css_rules())
        } else {
            None
        }
    }

    /// Insert one CSS rule into the aggregate document stylesheet.
    pub fn insert_style_sheet_rule(
        &mut self,
        sheet_index: usize,
        css_text: &str,
        rule_index: usize,
    ) -> Result<usize, String> {
        if sheet_index != 0 {
            return Err("IndexSizeError".to_string());
        }
        let inserted = self.stylesheet.insert_author_rule(css_text, rule_index)?;
        self.style_dirty = true;
        Ok(inserted)
    }

    /// Delete one CSS rule from the aggregate document stylesheet.
    pub fn delete_style_sheet_rule(
        &mut self,
        sheet_index: usize,
        rule_index: usize,
    ) -> Result<(), String> {
        if sheet_index != 0 {
            return Err("IndexSizeError".to_string());
        }
        self.stylesheet.delete_rule(rule_index)?;
        self.style_dirty = true;
        Ok(())
    }

    // ── Layout queries ──

    fn raw_border_rect_document(&self, id: u32) -> Option<Rect> {
        let node = self.get_node(id).or_else(|| self.find_webcore(id))?;
        Some(node.layout.border_rect)
    }

    fn raw_padding_rect_document(&self, id: u32) -> Option<Rect> {
        let node = self.get_node(id).or_else(|| self.find_webcore(id))?;
        Some(node.layout.padding_rect)
    }

    fn bounding_border_rect_document(&self, id: u32) -> Option<Rect> {
        // `get_node` walks the LIGHT tree only. A shadow node is a real box
        // with a real rect — it is just not reachable from `root.children` —
        // so the shadow-aware lookup is the fallback rather than a second
        // answer for the same node.
        let node = self.get_node(id).or_else(|| self.find_webcore(id))?;
        let rect = node.layout.border_rect;
        // ⛔ The TRANSFORMED border box — CSSOM View §4 says this returns the
        // bounding rectangle of the element's transformed border boxes, and
        // this answered the untransformed one, so a page could not find out
        // where a transformed element actually is.
        //
        // Costs nothing on the overwhelmingly common path: no transform, no
        // work. Only the element's OWN transform is composed — an ancestor's
        // is not, which is a further step.
        if node.style.transform.is_empty() || node.style.transform == "none" {
            return Some(rect);
        }
        let m = crate::renderer::display_list_builder::compute_transform_matrix(
            &node.style,
            &rect,
            &crate::types::TransformCtx {
                font_px: node.style.font_size_px(16.0, 16.0),
                root_font_px: 16.0,
                viewport_w: self.viewport_w,
                viewport_h: self.viewport_h,
            },
        );
        let pt = |x: f32, y: f32| (m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]);
        let corners = [
            pt(rect.x, rect.y),
            pt(rect.x + rect.w, rect.y),
            pt(rect.x, rect.y + rect.h),
            pt(rect.x + rect.w, rect.y + rect.h),
        ];
        let (mut x0, mut y0) = (f32::INFINITY, f32::INFINITY);
        let (mut x1, mut y1) = (f32::NEG_INFINITY, f32::NEG_INFINITY);
        for (x, y) in corners {
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
        Some(Rect::new(x0, y0, x1 - x0, y1 - y0))
    }

    /// Get the bounding rect of a node (border box in viewport coordinates).
    pub fn get_bounding_client_rect(&self, id: u32) -> Option<Rect> {
        self.bounding_border_rect_document(id).map(|mut rect| {
            rect.x -= self.scroll_x;
            rect.y -= self.scroll_y;
            rect
        })
    }

    /// Get the offset width (border box width).
    pub fn offset_width(&self, id: u32) -> f32 {
        self.raw_border_rect_document(id)
            .map(|r| r.w)
            .unwrap_or(0.0)
    }

    /// Get the offset height (border box height).
    pub fn offset_height(&self, id: u32) -> f32 {
        self.raw_border_rect_document(id)
            .map(|r| r.h)
            .unwrap_or(0.0)
    }

    /// `element.clientTop` — top border width.
    pub fn client_top(&self, id: u32) -> f32 {
        self.find_webcore(id)
            .map(|node| node.layout.resolved_border_top)
            .unwrap_or(0.0)
    }

    /// `element.clientLeft` — left border width.
    pub fn client_left(&self, id: u32) -> f32 {
        self.find_webcore(id)
            .map(|node| node.layout.resolved_border_left)
            .unwrap_or(0.0)
    }

    /// `element.clientWidth` — padding box width, excluding borders.
    pub fn client_width(&self, id: u32) -> f32 {
        self.raw_padding_rect_document(id)
            .map(|r| r.w)
            .unwrap_or(0.0)
    }

    /// `element.clientHeight` — padding box height, excluding borders.
    pub fn client_height(&self, id: u32) -> f32 {
        self.raw_padding_rect_document(id)
            .map(|r| r.h)
            .unwrap_or(0.0)
    }

    /// `element.scrollTop`.
    pub fn element_scroll_top(&self, id: u32) -> f32 {
        self.find_webcore(id)
            .map(|node| node.layout.scroll_top)
            .unwrap_or(0.0)
    }

    /// `element.scrollLeft`.
    pub fn element_scroll_left(&self, id: u32) -> f32 {
        self.find_webcore(id)
            .map(|node| node.layout.scroll_left)
            .unwrap_or(0.0)
    }

    /// `element.scrollWidth`.
    pub fn element_scroll_width(&self, id: u32) -> f32 {
        self.find_webcore(id)
            .map(|node| node.layout.scroll_width)
            .unwrap_or(0.0)
    }

    /// `element.scrollHeight`.
    pub fn element_scroll_height(&self, id: u32) -> f32 {
        self.find_webcore(id)
            .map(|node| node.layout.scroll_height)
            .unwrap_or(0.0)
    }

    /// `element.scrollTo(x, y)`.
    pub fn element_scroll_to(&mut self, id: u32, x: f32, y: f32) {
        let max_x = (self.element_scroll_width(id) - self.client_width(id)).max(0.0);
        let max_y = (self.element_scroll_height(id) - self.client_height(id)).max(0.0);
        let old_x = self.element_scroll_left(id);
        let old_y = self.element_scroll_top(id);
        let target_x = if x.is_finite() {
            x.clamp(0.0, max_x)
        } else {
            0.0
        };
        let target_y = if y.is_finite() {
            y.clamp(0.0, max_y)
        } else {
            0.0
        };
        let smooth = self
            .find_webcore(id)
            .is_some_and(|node| node.style.scroll_behavior == crate::types::ScrollBehavior::Smooth);
        if smooth {
            if (target_x - old_x).abs() <= 0.01 && (target_y - old_y).abs() <= 0.01 {
                return;
            }
            self.smooth_scrolls.retain(|s| s.element_id != id);
            self.smooth_scrolls.push(crate::types::SmoothScrollState {
                element_id: id,
                start_x: old_x,
                start_y: old_y,
                target_x,
                target_y,
                start_time: std::time::Instant::now(),
                duration: std::time::Duration::from_millis(250),
            });
            self.needs_animation_frame = true;
            return;
        }
        if let Some(node) = self.find_webcore_mut(id) {
            node.layout.scroll_left = target_x;
            node.layout.scroll_top = target_y;
        }
        if (self.element_scroll_left(id) - old_x).abs() > 0.01
            || (self.element_scroll_top(id) - old_y).abs() > 0.01
        {
            let mut event = crate::dom::events::DomEvent::new("scroll", id);
            self.dispatch_dom_event(&mut event);
        }
    }

    /// `element.scrollBy(dx, dy)`.
    pub fn element_scroll_by(&mut self, id: u32, dx: f32, dy: f32) {
        let x = self.element_scroll_left(id) + dx;
        let y = self.element_scroll_top(id) + dy;
        self.element_scroll_to(id, x, y);
    }

    /// Programmatic viewport scroll. Honors root `scroll-behavior:smooth`.
    pub fn viewport_scroll_to(&mut self, x: f32, y: f32, viewport_w: f32, viewport_h: f32) -> bool {
        let doc_h =
            crate::types::Document::scroll_height(&self.root).max(self.root.layout.margin_rect.h);
        let doc_w = self.root.layout.margin_rect.w;
        let old_x = self.scroll_x;
        let old_y = self.scroll_y;
        let target_x = x.max(0.0).min((doc_w - viewport_w).max(0.0));
        let target_y = if self.viewport_y_scroll_locked() {
            self.scroll_y
        } else {
            y.max(0.0).min((doc_h - viewport_h).max(0.0))
        };
        if (target_x - old_x).abs() <= 0.01 && (target_y - old_y).abs() <= 0.01 {
            return false;
        }
        if self.root.style.scroll_behavior == crate::types::ScrollBehavior::Smooth {
            self.smooth_scrolls.retain(|s| s.element_id != 0);
            self.smooth_scrolls.push(crate::types::SmoothScrollState {
                element_id: 0,
                start_x: old_x,
                start_y: old_y,
                target_x,
                target_y,
                start_time: std::time::Instant::now(),
                duration: std::time::Duration::from_millis(250),
            });
            self.needs_animation_frame = true;
            return true;
        }
        self.scroll_x = target_x;
        self.scroll_y = target_y;
        self.fire_window_event("scroll");
        true
    }

    pub fn tick_smooth_scrolls(&mut self, now: std::time::Instant) {
        if self.smooth_scrolls.is_empty() {
            return;
        }
        let mut remaining = Vec::with_capacity(self.smooth_scrolls.len());
        let mut scrolled = Vec::new();
        for state in std::mem::take(&mut self.smooth_scrolls) {
            let denom = state.duration.as_secs_f32().max(0.001);
            let t = (now.duration_since(state.start_time).as_secs_f32() / denom).clamp(0.0, 1.0);
            let eased = t * t * (3.0 - 2.0 * t);
            let x = state.start_x + (state.target_x - state.start_x) * eased;
            let y = state.start_y + (state.target_y - state.start_y) * eased;
            if state.element_id == 0 {
                let old_x = self.scroll_x;
                let old_y = self.scroll_y;
                self.scroll_x = x;
                self.scroll_y = y;
                if (self.scroll_x - old_x).abs() > 0.01 || (self.scroll_y - old_y).abs() > 0.01 {
                    scrolled.push(0);
                }
            } else if let Some(node) = self.find_webcore_mut(state.element_id) {
                let old_x = node.layout.scroll_left;
                let old_y = node.layout.scroll_top;
                node.layout.scroll_left = x;
                node.layout.scroll_top = y;
                if (node.layout.scroll_left - old_x).abs() > 0.01
                    || (node.layout.scroll_top - old_y).abs() > 0.01
                {
                    scrolled.push(state.element_id);
                }
            }
            if t < 1.0 {
                remaining.push(state);
            }
        }
        self.smooth_scrolls = remaining;
        for id in scrolled {
            if id == 0 {
                self.fire_window_event("scroll");
            } else {
                let mut event = crate::dom::events::DomEvent::new("scroll", id);
                self.dispatch_dom_event(&mut event);
            }
        }
        if !self.smooth_scrolls.is_empty() {
            self.needs_animation_frame = true;
        }
    }

    /// `document.elementFromPoint(x, y)` — viewport coordinates to topmost element.
    pub fn element_from_point(&self, x: f32, y: f32) -> Option<u32> {
        if !x.is_finite() || !y.is_finite() || x < 0.0 || y < 0.0 {
            return None;
        }
        crate::layout::hit_test::point_to_hit(&self.root, (x + self.scroll_x, y + self.scroll_y), 0)
            .map(|hit| hit.node_id)
    }

    /// `window.matchMedia(query)`/`MediaQueryList.matches`.
    pub fn match_media(&self, query: &str) -> MediaQueryList {
        MediaQueryList {
            media: query.to_string(),
            matches: crate::css::media_query::evaluate_media(
                query,
                self.viewport_w,
                self.viewport_h,
            ),
        }
    }

    /// SVGAnimationElement.beginElement().
    pub fn svg_begin_element(&mut self, id: u32) -> bool {
        self.svg_begin_element_at(id, 0.0)
    }

    /// SVGAnimationElement.beginElementAt(offset).
    pub fn svg_begin_element_at(&mut self, id: u32, offset_s: f32) -> bool {
        self.push_svg_animation_control(id, "begin", offset_s)
    }

    /// SVGAnimationElement.endElement().
    pub fn svg_end_element(&mut self, id: u32) -> bool {
        self.svg_end_element_at(id, 0.0)
    }

    /// SVGAnimationElement.endElementAt(offset).
    pub fn svg_end_element_at(&mut self, id: u32, offset_s: f32) -> bool {
        self.push_svg_animation_control(id, "end", offset_s)
    }

    fn push_svg_animation_control(&mut self, id: u32, kind: &str, offset_s: f32) -> bool {
        let Some(animation_path) = self
            .find_webcore(id)
            .filter(|node| is_svg_animation_dom_tag(&node.tag))
            .and_then(|node| node.svg_tree_path.clone())
        else {
            return false;
        };
        let now = std::time::Instant::now();
        let Some(svg_root) = find_svg_owner_for_node_mut(&mut self.root, id) else {
            return false;
        };
        let start = *svg_root.svg_animation_start_time.get_or_insert(now);
        let elapsed = now.duration_since(start).as_secs_f32();
        svg_root.svg_animation_controls.push((
            animation_path,
            kind.to_string(),
            elapsed + offset_s,
        ));
        true
    }

    /// Queue SVG eventbase instance times for animations targeting this node.
    pub(crate) fn svg_trigger_event(&mut self, target_id: u32, event_name: &str) -> bool {
        let Some(event_target_path) = self
            .find_webcore(target_id)
            .and_then(|node| node.svg_tree_path.clone())
        else {
            return false;
        };

        let now = std::time::Instant::now();
        let Some(svg_root) = find_svg_owner_for_node_mut(&mut self.root, target_id) else {
            return false;
        };
        let Some(doc) = svg_root.svg_document.clone() else {
            return false;
        };
        let start = *svg_root.svg_animation_start_time.get_or_insert(now);
        let elapsed = now.duration_since(start).as_secs_f32();
        let controls = crate::svg::animation::animation_controls_for_event(
            &doc,
            &event_target_path,
            event_name,
            elapsed,
        );
        if controls.is_empty() {
            return false;
        }
        svg_root.svg_animation_controls.extend(controls);
        true
    }

    /// Queue document-level SVG `accessKey(...)` SMIL instance times.
    pub(crate) fn svg_trigger_access_key(&mut self, key: &str) -> bool {
        if key.is_empty() {
            return false;
        }
        let now = std::time::Instant::now();
        let mut queued = false;

        fn walk(node: &mut crate::types::WebCore, key: &str, now: std::time::Instant) -> bool {
            let mut queued = false;
            if let Some(doc) = node.svg_document.clone() {
                let start = *node.svg_animation_start_time.get_or_insert(now);
                let elapsed = now.duration_since(start).as_secs_f32();
                let controls =
                    crate::svg::animation::animation_controls_for_access_key(&doc, key, elapsed);
                if !controls.is_empty() {
                    node.svg_animation_controls.extend(controls);
                    queued = true;
                }
            }
            for child in &mut node.children {
                queued |= walk(child, key, now);
            }
            if let Some(shadow) = &mut node.shadow_root {
                for child in &mut shadow.children {
                    queued |= walk(child, key, now);
                }
            }
            queued
        }

        queued |= walk(&mut self.root, key, now);
        queued
    }

    // ── Internal helpers ──

    /// Find a shared reference to an WebCore by node_id.
    ///
    /// A tree walk on purpose. `get_box_by_id` has an O(1) fast path through
    /// `node_index`, which is a `HashMap<u32, *const WebCore>` rebuilt only by
    /// `rebuild_node_index()` — that is, only at layout. Any DOM mutation in
    /// between moves boxes inside their parent's `Vec<WebCore>` and leaves
    /// those pointers dangling, and the fast path would hand one back. The DOM
    /// API mutates without laying out, so it must not use that index.
    pub(crate) fn find_webcore(&self, id: u32) -> Option<&WebCore> {
        // `pending_nodes` FIRST. A node created but not yet inserted is not in
        // the tree, and the ordinary DOM idiom writes to it before it ever is:
        //
        //     el = createElement(); el.setAttribute(..); parent.appendChild(el)
        //
        // Searching only `root` made every such write vanish from the render
        // box while still reaching the arena — so `getAttribute` (arena) agreed
        // and `getElementById` (render tree) did not.
        if self.pending_nodes.contains_key(&id) {
            return self.pending_nodes.get(&id);
        }
        // **A DETACHED SUBTREE IS STILL A TREE.**
        //
        // `pending_nodes` holds detached ROOTS. The moment a node is appended
        // into one it stops being a root and lives in that box's `children`,
        // so a map lookup no longer finds it — and neither does a walk from
        // `root`, because none of it is in the document yet.
        //
        // That is the ordinary idiom one level deeper, and it is what
        // `innerHTML` on a detached element does for every tag after the
        // first: build the subtree, then insert it. Without this the parser
        // could attach a control's direct children and nothing below them —
        // a `<table>` kept its `<thead>` nowhere, and a `<button>`'s own TEXT
        // was dropped, so composed chrome came out as empty boxes.
        fn find_pending<'a>(nodes: &'a [WebCore], id: u32) -> Option<&'a WebCore> {
            for node in nodes {
                if node.node_id == id {
                    return Some(node);
                }
                if let Some(found) = find_pending(&node.children, id) {
                    return Some(found);
                }
            }
            None
        }
        for pending in self.pending_nodes.values() {
            if let Some(found) = find_pending(std::slice::from_ref(pending), id) {
                return Some(found);
            }
        }
        // A SHADOW TREE IS PART OF THE TREE.
        //
        // The walk followed `children` only, so no node inside a shadow root
        // was findable — and every API that resolves an id through here
        // (`get_attribute`, `tag_name`, `text_content`, the slot interface)
        // silently answered nothing for shadow content. It is the same class of
        // miss as the detached-subtree case above: a node that is genuinely in
        // the tree, reached by a link the walk did not follow.
        fn walk(node: &WebCore, id: u32) -> Option<&WebCore> {
            if node.node_id == id {
                return Some(node);
            }
            if let Some(sr) = &node.shadow_root {
                for child in &sr.children {
                    if let Some(found) = walk(child, id) {
                        return Some(found);
                    }
                }
            }
            for child in &node.children {
                if let Some(found) = walk(child, id) {
                    return Some(found);
                }
            }
            None
        }
        walk(&self.root, id)
    }

    /// Find a mutable reference to an WebCore by node_id.
    ///
    /// Checks `pending_nodes` first, for the reason spelled out on
    /// `find_webcore` — a detached node is still a legal target for
    /// `setAttribute`, `setTextContent` and a style write.
    pub(crate) fn find_webcore_mut(&mut self, id: u32) -> Option<&mut WebCore> {
        if self.pending_nodes.contains_key(&id) {
            return self.pending_nodes.get_mut(&id);
        }
        // Shadow trees too — see the note in `find_webcore`.
        fn walk(node: &mut WebCore, id: u32) -> Option<&mut WebCore> {
            if node.node_id == id {
                return Some(node);
            }
            if let Some(sr) = node.shadow_root.as_mut() {
                for child in &mut sr.children {
                    if let Some(found) = walk(child, id) {
                        return Some(found);
                    }
                }
            }
            for child in &mut node.children {
                if let Some(found) = walk(child, id) {
                    return Some(found);
                }
            }
            None
        }
        // The same detached-subtree search `find_webcore` explains, and the
        // half that actually loses nodes: `append_child` takes the child OUT of
        // `pending_nodes` before asking for its parent, so a parent this cannot
        // find means the child is already gone and is dropped in silence.
        //
        // Found in two passes because the owning root has to be identified
        // before the map can be borrowed mutably to walk into it.
        fn contains(node: &WebCore, id: u32) -> bool {
            node.node_id == id
                || node
                    .shadow_root
                    .as_ref()
                    .map(|sr| sr.children.iter().any(|c| contains(c, id)))
                    .unwrap_or(false)
                || node.children.iter().any(|child| contains(child, id))
        }
        let owner = self
            .pending_nodes
            .iter()
            .find(|(_, pending)| contains(pending, id))
            .map(|(key, _)| *key);
        if let Some(owner) = owner {
            return self
                .pending_nodes
                .get_mut(&owner)
                .and_then(|pending| walk(pending, id));
        }
        walk(&mut self.root, id)
    }

    /// Detach an WebCore from its parent in the tree, returning the detached box.
    fn detach_webcore(&mut self, id: u32) -> Option<WebCore> {
        fn walk(node: &mut WebCore, id: u32) -> Option<WebCore> {
            if let Some(idx) = node.children.iter().position(|c| c.node_id == id) {
                return Some(node.children.remove(idx));
            }
            for child in &mut node.children {
                if let Some(found) = walk(child, id) {
                    return Some(found);
                }
            }
            None
        }
        walk(&mut self.root, id)
    }
}

// ─── Helpers ────────────────────────────────────────────────────────────────

fn is_svg_animation_dom_tag(tag: &str) -> bool {
    matches!(
        tag,
        "animate"
            | "animatecolor"
            | "animateColor"
            | "animatetransform"
            | "animateTransform"
            | "animatemotion"
            | "animateMotion"
            | "set"
    )
}

fn find_svg_owner_for_node_mut(node: &mut WebCore, id: u32) -> Option<&mut WebCore> {
    if node.svg_document.is_some() && webcore_contains_node_id(node, id) {
        return Some(node);
    }
    for child in &mut node.children {
        if let Some(found) = find_svg_owner_for_node_mut(child, id) {
            return Some(found);
        }
    }
    None
}

fn webcore_contains_node_id(node: &WebCore, id: u32) -> bool {
    if node.node_id == id {
        return true;
    }
    if node
        .shadow_root
        .as_ref()
        .map(|shadow| {
            shadow
                .children
                .iter()
                .any(|child| webcore_contains_node_id(child, id))
        })
        .unwrap_or(false)
    {
        return true;
    }
    node.children
        .iter()
        .any(|child| webcore_contains_node_id(child, id))
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct InlineStyleDecl {
    name: String,
    value: String,
    important: bool,
}

fn parse_inline_style(style: &str) -> Vec<InlineStyleDecl> {
    crate::css::parser::split_declarations(style)
        .into_iter()
        .filter_map(|decl| {
            let decl = decl.trim();
            if decl.is_empty() {
                return None;
            }
            let colon = decl.find(':')?;
            let name = decl[..colon].trim().to_ascii_lowercase();
            let (value, important) = split_important_suffix(decl[colon + 1..].trim());
            Some(InlineStyleDecl {
                name,
                value,
                important,
            })
        })
        .collect()
}

fn split_important_suffix(value: &str) -> (String, bool) {
    let value = value.trim();
    let suffix = "!important";
    if value.len() >= suffix.len()
        && value[value.len() - suffix.len()..].eq_ignore_ascii_case(suffix)
    {
        (
            value[..value.len() - suffix.len()].trim_end().to_string(),
            true,
        )
    } else {
        (value.to_string(), false)
    }
}

fn serialize_inline_style(props: &[InlineStyleDecl]) -> String {
    props
        .iter()
        .map(|decl| {
            if decl.important {
                format!("{}: {} !important;", decl.name, decl.value)
            } else {
                format!("{}: {};", decl.name, decl.value)
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn inline_shorthand_longhands(prop: &str) -> Option<&'static [&'static str]> {
    const BORDER_LONGHANDS: &[&str] = &[
        "border-top-width",
        "border-right-width",
        "border-bottom-width",
        "border-left-width",
        "border-top-style",
        "border-right-style",
        "border-bottom-style",
        "border-left-style",
        "border-top-color",
        "border-right-color",
        "border-bottom-color",
        "border-left-color",
    ];
    const FONT_LONGHANDS: &[&str] = &[
        "font-family",
        "font-size",
        "font-weight",
        "font-style",
        "font-stretch",
        "line-height",
        "font-variant",
        "font-variant-alternates",
        "font-variant-caps",
        "font-variant-east-asian",
        "font-variant-emoji",
        "font-variant-ligatures",
        "font-variant-numeric",
        "font-variant-position",
        "font-feature-settings",
        "font-variation-settings",
    ];
    const BACKGROUND_LONGHANDS: &[&str] = &[
        "background-color",
        "background-image",
        "background-position",
        "background-size",
        "background-repeat",
        "background-attachment",
        "background-origin",
        "background-clip",
        "background-blend-mode",
    ];
    const LIST_STYLE_LONGHANDS: &[&str] =
        &["list-style-type", "list-style-position", "list-style-image"];
    const TRANSITION_LONGHANDS: &[&str] = &[
        "transition-property",
        "transition-duration",
        "transition-timing-function",
        "transition-delay",
        "transition-behavior",
    ];
    const ANIMATION_LONGHANDS: &[&str] = &[
        "animation-name",
        "animation-duration",
        "animation-timing-function",
        "animation-delay",
        "animation-iteration-count",
        "animation-direction",
        "animation-fill-mode",
        "animation-play-state",
        "animation-composition",
    ];
    const MASK_LONGHANDS: &[&str] = &[
        "mask-image",
        "mask-mode",
        "mask-repeat",
        "mask-position",
        "mask-size",
        "mask-clip",
        "mask-origin",
        "mask-composite",
    ];
    const BORDER_IMAGE_LONGHANDS: &[&str] = &[
        "border-image-source",
        "border-image-slice",
        "border-image-width",
        "border-image-outset",
        "border-image-repeat",
    ];
    const OUTLINE_LONGHANDS: &[&str] = &["outline-width", "outline-style", "outline-color"];
    const COLUMNS_LONGHANDS: &[&str] = &["column-width", "column-count"];
    const COLUMN_RULE_LONGHANDS: &[&str] = &[
        "column-rule-width",
        "column-rule-style",
        "column-rule-color",
    ];
    Some(match prop {
        "margin" => &["margin-top", "margin-right", "margin-bottom", "margin-left"],
        "padding" => &[
            "padding-top",
            "padding-right",
            "padding-bottom",
            "padding-left",
        ],
        "inset" => &["top", "right", "bottom", "left"],
        "gap" => &["row-gap", "column-gap"],
        "border" => BORDER_LONGHANDS,
        "font" => FONT_LONGHANDS,
        "background" => BACKGROUND_LONGHANDS,
        "list-style" => LIST_STYLE_LONGHANDS,
        "transition" => TRANSITION_LONGHANDS,
        "animation" => ANIMATION_LONGHANDS,
        "mask" => MASK_LONGHANDS,
        "border-image" => BORDER_IMAGE_LONGHANDS,
        "outline" => OUTLINE_LONGHANDS,
        "columns" => COLUMNS_LONGHANDS,
        "column-rule" => COLUMN_RULE_LONGHANDS,
        "border-top" => &["border-top-width", "border-top-style", "border-top-color"],
        "border-right" => &[
            "border-right-width",
            "border-right-style",
            "border-right-color",
        ],
        "border-bottom" => &[
            "border-bottom-width",
            "border-bottom-style",
            "border-bottom-color",
        ],
        "border-left" => &[
            "border-left-width",
            "border-left-style",
            "border-left-color",
        ],
        _ => return None,
    })
}

fn expand_inline_shorthand(prop: &str, value: &str) -> Option<Vec<InlineStyleDecl>> {
    let (value, important) = split_important_suffix(value);
    let toks = crate::css::split_shorthand_values(&value);
    if toks.is_empty() {
        return None;
    }
    let make = |name: &str, value: &str| InlineStyleDecl {
        name: name.to_string(),
        value: value.to_string(),
        important,
    };
    Some(match prop {
        "margin" | "padding" | "inset" => {
            if toks.len() > 4 {
                return None;
            }
            let top = toks[0];
            let right = toks.get(1).copied().unwrap_or(top);
            let bottom = toks.get(2).copied().unwrap_or(top);
            let left = toks.get(3).copied().unwrap_or(right);
            let names = inline_shorthand_longhands(prop)?;
            vec![
                make(names[0], top),
                make(names[1], right),
                make(names[2], bottom),
                make(names[3], left),
            ]
        }
        "gap" => {
            if toks.len() > 2 {
                return None;
            }
            vec![
                make("row-gap", toks[0]),
                make("column-gap", toks.get(1).copied().unwrap_or(toks[0])),
            ]
        }
        "font" => expand_inline_font_shorthand(&value, important)?,
        "background" => expand_inline_background_shorthand(&value, important)?,
        "list-style" => expand_inline_list_style_shorthand(&value, important),
        "transition" => expand_inline_transition_shorthand(&value, important),
        "animation" => expand_inline_animation_shorthand(&value, important),
        "mask" => expand_inline_mask_shorthand(&value, important),
        "border-image" => expand_inline_border_image_shorthand(&value, important),
        "outline" => expand_inline_outline_shorthand(&value, important),
        "columns" => expand_inline_columns_shorthand(&value, important),
        "column-rule" => expand_inline_column_rule_shorthand(&value, important),
        "border" => {
            let (width, style, color) = parse_inline_border_components(&toks);
            let sides = ["top", "right", "bottom", "left"];
            let mut decls = Vec::with_capacity(12);
            for part in ["width", "style", "color"] {
                let value = match part {
                    "width" => width,
                    "style" => style,
                    _ => color,
                };
                for side in sides {
                    decls.push(make(&format!("border-{side}-{part}"), value));
                }
            }
            decls
        }
        "border-top" | "border-right" | "border-bottom" | "border-left" => {
            let (width, style, color) = parse_inline_border_components(&toks);
            let side = prop.trim_start_matches("border-");
            vec![
                make(&format!("border-{side}-width"), width),
                make(&format!("border-{side}-style"), style),
                make(&format!("border-{side}-color"), color),
            ]
        }
        _ => return None,
    })
}

fn expand_inline_background_shorthand(
    value: &str,
    important: bool,
) -> Option<Vec<InlineStyleDecl>> {
    use crate::types::{ComputedStyle, GradientType};

    let mut style = ComputedStyle::default();
    crate::css::apply_property(&mut style, "background", value);
    if style.gradient_type != GradientType::None {
        return None;
    }

    let make = |name: &str, value: String| InlineStyleDecl {
        name: name.to_string(),
        value,
        important,
    };
    Some(vec![
        make(
            "background-color",
            serialize_inline_color(style.background_color),
        ),
        make(
            "background-image",
            serialize_inline_background_image(&style.background_image_url),
        ),
        make(
            "background-position",
            format!(
                "{} {}",
                crate::html::serializer::serialize_length(&style.background_position_x),
                crate::html::serializer::serialize_length(&style.background_position_y)
            ),
        ),
        make("background-size", serialize_inline_background_size(&style)),
        make(
            "background-repeat",
            serialize_inline_background_repeat(style.background_repeat),
        ),
        make(
            "background-attachment",
            serialize_inline_background_attachment(style.background_attachment),
        ),
        make(
            "background-origin",
            serialize_inline_background_clip(style.background_origin),
        ),
        make(
            "background-clip",
            serialize_inline_background_clip(style.background_clip),
        ),
        make("background-blend-mode", style.background_blend_mode.clone()),
    ])
}

fn expand_inline_list_style_shorthand(value: &str, important: bool) -> Vec<InlineStyleDecl> {
    let mut style = crate::types::ComputedStyle::default();
    crate::css::apply_property(&mut style, "list-style", value);

    let make = |name: &str, value: String| InlineStyleDecl {
        name: name.to_string(),
        value,
        important,
    };
    vec![
        make(
            "list-style-type",
            if style.custom_list_style_type.is_empty() {
                serialize_inline_list_style_type(style.list_style_type)
            } else {
                style.custom_list_style_type.clone()
            },
        ),
        make(
            "list-style-position",
            serialize_inline_list_style_position(style.list_style_position),
        ),
        make(
            "list-style-image",
            serialize_inline_list_style_image(&style.list_style_image),
        ),
    ]
}

fn expand_inline_transition_shorthand(value: &str, important: bool) -> Vec<InlineStyleDecl> {
    let transitions = crate::css::parse_transition_shorthand(value);
    let make = |name: &str, value: String| InlineStyleDecl {
        name: name.to_string(),
        value,
        important,
    };
    if transitions.is_empty() {
        return vec![
            make("transition-property", "all".to_string()),
            make("transition-duration", "0s".to_string()),
            make("transition-timing-function", "ease".to_string()),
            make("transition-delay", "0s".to_string()),
            make("transition-behavior", "normal".to_string()),
        ];
    }
    let join = |values: Vec<String>| values.join(", ");
    vec![
        make(
            "transition-property",
            join(transitions.iter().map(|tr| tr.property.clone()).collect()),
        ),
        make(
            "transition-duration",
            join(
                transitions
                    .iter()
                    .map(|tr| serialize_inline_time_ms(tr.duration_ms))
                    .collect(),
            ),
        ),
        make(
            "transition-timing-function",
            join(
                transitions
                    .iter()
                    .map(|tr| serialize_inline_easing(&tr.timing_fn))
                    .collect(),
            ),
        ),
        make(
            "transition-delay",
            join(
                transitions
                    .iter()
                    .map(|tr| serialize_inline_time_ms(tr.delay_ms))
                    .collect(),
            ),
        ),
        make(
            "transition-behavior",
            join(
                transitions
                    .iter()
                    .map(|tr| {
                        if tr.allow_discrete {
                            "allow-discrete".to_string()
                        } else {
                            "normal".to_string()
                        }
                    })
                    .collect(),
            ),
        ),
    ]
}

fn expand_inline_animation_shorthand(value: &str, important: bool) -> Vec<InlineStyleDecl> {
    let animations = crate::css::parse_animation_shorthand(value);
    let make = |name: &str, value: String| InlineStyleDecl {
        name: name.to_string(),
        value,
        important,
    };
    if animations.is_empty() {
        return vec![
            make("animation-name", "none".to_string()),
            make("animation-duration", "0s".to_string()),
            make("animation-timing-function", "ease".to_string()),
            make("animation-delay", "0s".to_string()),
            make("animation-iteration-count", "1".to_string()),
            make("animation-direction", "normal".to_string()),
            make("animation-fill-mode", "none".to_string()),
            make("animation-play-state", "running".to_string()),
            make("animation-composition", "replace".to_string()),
        ];
    }

    let join = |values: Vec<String>| values.join(", ");
    vec![
        make(
            "animation-name",
            join(animations.iter().map(|anim| anim.name.clone()).collect()),
        ),
        make(
            "animation-duration",
            join(
                animations
                    .iter()
                    .map(|anim| serialize_inline_time_ms(anim.duration_ms))
                    .collect(),
            ),
        ),
        make(
            "animation-timing-function",
            join(
                animations
                    .iter()
                    .map(|anim| serialize_inline_easing(&anim.timing_fn))
                    .collect(),
            ),
        ),
        make(
            "animation-delay",
            join(
                animations
                    .iter()
                    .map(|anim| serialize_inline_time_ms(anim.delay_ms))
                    .collect(),
            ),
        ),
        make(
            "animation-iteration-count",
            join(
                animations
                    .iter()
                    .map(|anim| serialize_inline_iteration_count(anim.iteration_count))
                    .collect(),
            ),
        ),
        make(
            "animation-direction",
            join(
                animations
                    .iter()
                    .map(|anim| serialize_inline_animation_direction(&anim.direction).to_string())
                    .collect(),
            ),
        ),
        make(
            "animation-fill-mode",
            join(
                animations
                    .iter()
                    .map(|anim| serialize_inline_animation_fill_mode(&anim.fill_mode).to_string())
                    .collect(),
            ),
        ),
        make(
            "animation-play-state",
            join(
                animations
                    .iter()
                    .map(|anim| {
                        if anim.play_state_paused {
                            "paused".to_string()
                        } else {
                            "running".to_string()
                        }
                    })
                    .collect(),
            ),
        ),
        make(
            "animation-composition",
            join(
                animations
                    .iter()
                    .map(|anim| {
                        serialize_inline_animation_composition(&anim.composition).to_string()
                    })
                    .collect(),
            ),
        ),
    ]
}

fn expand_inline_mask_shorthand(value: &str, important: bool) -> Vec<InlineStyleDecl> {
    let mut style = crate::types::ComputedStyle::default();
    crate::css::apply_property(&mut style, "mask", value);
    let rare = style.rare();
    let make = |name: &str, value: String| InlineStyleDecl {
        name: name.to_string(),
        value,
        important,
    };
    vec![
        make(
            "mask-image",
            if rare.mask_image_url.is_empty() {
                "none".to_string()
            } else {
                format!(
                    "url(\"{}\")",
                    rare.mask_image_url
                        .replace('\\', "\\\\")
                        .replace('"', "\\\"")
                )
            },
        ),
        make("mask-mode", inline_rare_or(&rare.mask_mode, "match-source")),
        make("mask-repeat", inline_rare_or(&rare.mask_repeat, "repeat")),
        make(
            "mask-position",
            inline_rare_or(&rare.mask_position, "0% 0%"),
        ),
        make("mask-size", inline_rare_or(&rare.mask_size, "auto")),
        make("mask-clip", inline_rare_or(&rare.mask_clip, "border-box")),
        make(
            "mask-origin",
            inline_rare_or(&rare.mask_origin, "border-box"),
        ),
        make(
            "mask-composite",
            inline_rare_or(&rare.mask_composite, "add"),
        ),
    ]
}

fn inline_rare_or(value: &str, initial: &str) -> String {
    if value.is_empty() {
        initial.to_string()
    } else {
        value.to_string()
    }
}

fn expand_inline_border_image_shorthand(value: &str, important: bool) -> Vec<InlineStyleDecl> {
    let mut style = crate::types::ComputedStyle::default();
    crate::css::apply_property(&mut style, "border-image", value);
    let make = |name: &str, value: String| InlineStyleDecl {
        name: name.to_string(),
        value,
        important,
    };
    vec![
        make("border-image-source", style.border_image_source.clone()),
        make("border-image-slice", style.border_image_slice.clone()),
        make("border-image-width", style.border_image_width.clone()),
        make("border-image-outset", style.border_image_outset.clone()),
        make("border-image-repeat", style.border_image_repeat.clone()),
    ]
}

fn expand_inline_outline_shorthand(value: &str, important: bool) -> Vec<InlineStyleDecl> {
    let mut style = crate::types::ComputedStyle::default();
    crate::css::apply_property(&mut style, "outline", value);
    vec![
        InlineStyleDecl {
            name: "outline-width".to_string(),
            value: format!("{}px", trim_inline_float(style.outline_width)),
            important,
        },
        InlineStyleDecl {
            name: "outline-style".to_string(),
            value: serialize_inline_border_style(style.outline_style).to_string(),
            important,
        },
        InlineStyleDecl {
            name: "outline-color".to_string(),
            value: serialize_inline_color(style.outline_color),
            important,
        },
    ]
}

fn expand_inline_columns_shorthand(value: &str, important: bool) -> Vec<InlineStyleDecl> {
    let mut style = crate::types::ComputedStyle::default();
    crate::css::apply_property(&mut style, "columns", value);
    vec![
        InlineStyleDecl {
            name: "column-width".to_string(),
            value: crate::html::serializer::serialize_length(&style.column_width),
            important,
        },
        InlineStyleDecl {
            name: "column-count".to_string(),
            value: style
                .column_count
                .map(|count| count.to_string())
                .unwrap_or_else(|| "auto".to_string()),
            important,
        },
    ]
}

fn expand_inline_column_rule_shorthand(value: &str, important: bool) -> Vec<InlineStyleDecl> {
    let mut style = crate::types::ComputedStyle::default();
    crate::css::apply_property(&mut style, "column-rule", value);
    vec![
        InlineStyleDecl {
            name: "column-rule-width".to_string(),
            value: crate::html::serializer::serialize_length(&style.column_rule_width),
            important,
        },
        InlineStyleDecl {
            name: "column-rule-style".to_string(),
            value: serialize_inline_border_style(style.column_rule_style).to_string(),
            important,
        },
        InlineStyleDecl {
            name: "column-rule-color".to_string(),
            value: serialize_inline_color(style.column_rule_color),
            important,
        },
    ]
}

fn expand_inline_font_shorthand(value: &str, important: bool) -> Option<Vec<InlineStyleDecl>> {
    use crate::types::{ComputedStyle, FontStyle};

    if value.split_whitespace().count() == 1 {
        let lower = value.trim().to_ascii_lowercase();
        if matches!(
            lower.as_str(),
            "caption" | "icon" | "menu" | "message-box" | "small-caption" | "status-bar"
        ) {
            return None;
        }
    }

    if !inline_font_shorthand_has_size(value) {
        return None;
    }

    let mut style = ComputedStyle::default();
    crate::css::apply_property(&mut style, "font", value);

    let make = |name: &str, value: String| InlineStyleDecl {
        name: name.to_string(),
        value,
        important,
    };
    Some(vec![
        make(
            "font-family",
            serialize_inline_font_family_list(&style.font_family),
        ),
        make(
            "font-size",
            crate::html::serializer::serialize_length(&style.font_size),
        ),
        make("font-weight", style.font_weight.value().to_string()),
        make(
            "font-style",
            match style.font_style {
                FontStyle::Normal => "normal",
                FontStyle::Italic => "italic",
                FontStyle::Oblique => "oblique",
            }
            .to_string(),
        ),
        make(
            "font-stretch",
            serialize_inline_font_stretch(style.font_stretch),
        ),
        make(
            "line-height",
            crate::html::serializer::serialize_length(&style.line_height),
        ),
        make(
            "font-variant",
            if style.small_caps {
                "small-caps".to_string()
            } else {
                "normal".to_string()
            },
        ),
        make(
            "font-variant-alternates",
            style.font_variant_alternates.clone(),
        ),
        make("font-variant-caps", style.font_variant_caps.clone()),
        make(
            "font-variant-east-asian",
            style.font_variant_east_asian.clone(),
        ),
        make("font-variant-emoji", style.font_variant_emoji.clone()),
        make(
            "font-variant-ligatures",
            style.font_variant_ligatures.clone(),
        ),
        make("font-variant-numeric", style.font_variant_numeric.clone()),
        make("font-variant-position", style.font_variant_position.clone()),
        make(
            "font-feature-settings",
            serialize_inline_font_feature_settings(&style.rare().font_feature_settings),
        ),
        make(
            "font-variation-settings",
            serialize_inline_font_variation_settings(&style.rare().font_variation_settings),
        ),
    ])
}

fn inline_font_shorthand_has_size(value: &str) -> bool {
    value.split_whitespace().any(|tok| {
        let size = tok.split_once('/').map(|(size, _)| size).unwrap_or(tok);
        let lower = size.to_ascii_lowercase();
        matches!(
            lower.as_str(),
            "xx-small"
                | "x-small"
                | "small"
                | "medium"
                | "large"
                | "x-large"
                | "xx-large"
                | "xxx-large"
                | "smaller"
                | "larger"
        ) || crate::css::parse_length_checked(size).is_some()
    })
}

fn serialize_inline_font_family_list(value: &str) -> String {
    crate::css::split_font_families(value)
        .into_iter()
        .map(|family| {
            if is_inline_css_identifier(&family) {
                family
            } else {
                format!("\"{}\"", family.replace('\\', "\\\\").replace('"', "\\\""))
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn is_inline_css_identifier(value: &str) -> bool {
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    (first.is_ascii_alphabetic() || first == '_' || first == '-')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

fn serialize_inline_font_stretch(stretch: f32) -> String {
    match stretch {
        n if (n - 50.0).abs() < f32::EPSILON => "ultra-condensed".to_string(),
        n if (n - 62.5).abs() < f32::EPSILON => "extra-condensed".to_string(),
        n if (n - 75.0).abs() < f32::EPSILON => "condensed".to_string(),
        n if (n - 87.5).abs() < f32::EPSILON => "semi-condensed".to_string(),
        n if (n - 100.0).abs() < f32::EPSILON => "normal".to_string(),
        n if (n - 112.5).abs() < f32::EPSILON => "semi-expanded".to_string(),
        n if (n - 125.0).abs() < f32::EPSILON => "expanded".to_string(),
        n if (n - 150.0).abs() < f32::EPSILON => "extra-expanded".to_string(),
        n if (n - 200.0).abs() < f32::EPSILON => "ultra-expanded".to_string(),
        n => format!("{n}%"),
    }
}

fn serialize_inline_font_feature_settings(settings: &[(String, u32)]) -> String {
    if settings.is_empty() {
        return "normal".to_string();
    }
    settings
        .iter()
        .map(|(tag, value)| {
            if *value == 1 {
                format!("\"{}\"", tag.replace('\\', "\\\\").replace('"', "\\\""))
            } else {
                format!(
                    "\"{}\" {}",
                    tag.replace('\\', "\\\\").replace('"', "\\\""),
                    value
                )
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn serialize_inline_font_variation_settings(settings: &[(String, f32)]) -> String {
    if settings.is_empty() {
        return "normal".to_string();
    }
    settings
        .iter()
        .map(|(tag, value)| {
            format!(
                "\"{}\" {}",
                tag.replace('\\', "\\\\").replace('"', "\\\""),
                trim_inline_float(*value)
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn trim_inline_float(value: f32) -> String {
    let mut out = value.to_string();
    if out.contains('.') {
        while out.ends_with('0') {
            out.pop();
        }
        if out.ends_with('.') {
            out.pop();
        }
    }
    out
}

fn serialize_inline_color(color: crate::types::Color) -> String {
    if color.a == 255 {
        format!("rgb({}, {}, {})", color.r, color.g, color.b)
    } else {
        let alpha = (color.a as f32 / 255.0 * 100.0).round() / 100.0;
        format!("rgba({}, {}, {}, {})", color.r, color.g, color.b, alpha)
    }
}

fn serialize_inline_border_style(style: crate::types::BorderStyle) -> &'static str {
    match style {
        crate::types::BorderStyle::None => "none",
        crate::types::BorderStyle::Hidden => "hidden",
        crate::types::BorderStyle::Solid => "solid",
        crate::types::BorderStyle::Dashed => "dashed",
        crate::types::BorderStyle::Dotted => "dotted",
        crate::types::BorderStyle::Double => "double",
        crate::types::BorderStyle::Inset => "inset",
        crate::types::BorderStyle::Outset => "outset",
        crate::types::BorderStyle::Groove => "groove",
        crate::types::BorderStyle::Ridge => "ridge",
    }
}

fn serialize_inline_background_image(url: &str) -> String {
    if url.is_empty() {
        "none".to_string()
    } else {
        format!(
            "url(\"{}\")",
            url.replace('\\', "\\\\").replace('"', "\\\"")
        )
    }
}

fn serialize_inline_background_size(style: &crate::types::ComputedStyle) -> String {
    use crate::types::BackgroundSize;

    match style.background_size {
        BackgroundSize::Auto => "auto".to_string(),
        BackgroundSize::Cover => "cover".to_string(),
        BackgroundSize::Contain => "contain".to_string(),
        BackgroundSize::Explicit => {
            let width = crate::html::serializer::serialize_length(&style.background_size_w);
            let height = crate::html::serializer::serialize_length(&style.background_size_h);
            if height == "auto" {
                width
            } else {
                format!("{width} {height}")
            }
        }
    }
}

fn serialize_inline_background_repeat(repeat: crate::types::BackgroundRepeat) -> String {
    use crate::types::{BackgroundRepeat, BackgroundRepeatAxis};

    fn axis(axis: BackgroundRepeatAxis) -> &'static str {
        match axis {
            BackgroundRepeatAxis::Repeat => "repeat",
            BackgroundRepeatAxis::NoRepeat => "no-repeat",
            BackgroundRepeatAxis::Space => "space",
            BackgroundRepeatAxis::Round => "round",
        }
    }

    match repeat {
        BackgroundRepeat::Repeat => "repeat".to_string(),
        BackgroundRepeat::RepeatX => "repeat-x".to_string(),
        BackgroundRepeat::RepeatY => "repeat-y".to_string(),
        BackgroundRepeat::NoRepeat => "no-repeat".to_string(),
        BackgroundRepeat::Space => "space".to_string(),
        BackgroundRepeat::Round => "round".to_string(),
        BackgroundRepeat::TwoValue(x, y) => format!("{} {}", axis(x), axis(y)),
    }
}

fn serialize_inline_background_clip(clip: crate::types::BackgroundClip) -> String {
    use crate::types::BackgroundClip;

    match clip {
        BackgroundClip::BorderBox => "border-box",
        BackgroundClip::PaddingBox => "padding-box",
        BackgroundClip::ContentBox => "content-box",
        BackgroundClip::Text => "text",
    }
    .to_string()
}

fn serialize_inline_background_attachment(
    attachment: crate::types::BackgroundAttachment,
) -> String {
    use crate::types::BackgroundAttachment;

    match attachment {
        BackgroundAttachment::Scroll => "scroll",
        BackgroundAttachment::Fixed => "fixed",
        BackgroundAttachment::Local => "local",
    }
    .to_string()
}

fn serialize_inline_list_style_type(value: crate::types::ListStyleType) -> String {
    use crate::types::ListStyleType as L;

    match value {
        L::None => "none",
        L::Disc => "disc",
        L::Circle => "circle",
        L::Square => "square",
        L::Decimal => "decimal",
        L::DecimalLeadingZero => "decimal-leading-zero",
        L::LowerAlpha => "lower-alpha",
        L::UpperAlpha => "upper-alpha",
        L::LowerLatin => "lower-latin",
        L::UpperLatin => "upper-latin",
        L::LowerRoman => "lower-roman",
        L::UpperRoman => "upper-roman",
        L::LowerGreek => "lower-greek",
        L::Armenian => "armenian",
        L::Georgian => "georgian",
        L::Hebrew => "hebrew",
        L::Hiragana => "hiragana",
        L::Katakana => "katakana",
        L::HiraganaIroha => "hiragana-iroha",
        L::KatakanaIroha => "katakana-iroha",
        L::CjkDecimal => "cjk-decimal",
        L::Disclosure => "disclosure-open",
    }
    .to_string()
}

fn serialize_inline_list_style_position(value: crate::types::ListStylePosition) -> String {
    match value {
        crate::types::ListStylePosition::Outside => "outside",
        crate::types::ListStylePosition::Inside => "inside",
    }
    .to_string()
}

fn serialize_inline_list_style_image(url: &str) -> String {
    if url.is_empty() {
        "none".to_string()
    } else {
        format!(
            "url(\"{}\")",
            url.replace('\\', "\\\\").replace('"', "\\\"")
        )
    }
}

fn serialize_inline_time_ms(ms: f32) -> String {
    if (ms / 1000.0).fract().abs() < f32::EPSILON {
        format!("{}s", trim_inline_float(ms / 1000.0))
    } else {
        format!("{}ms", trim_inline_float(ms))
    }
}

fn serialize_inline_iteration_count(count: f32) -> String {
    if count.is_infinite() {
        "infinite".to_string()
    } else {
        trim_inline_float(count)
    }
}

fn serialize_inline_animation_direction(direction: &crate::types::AnimDirection) -> &'static str {
    match direction {
        crate::types::AnimDirection::Normal => "normal",
        crate::types::AnimDirection::Reverse => "reverse",
        crate::types::AnimDirection::Alternate => "alternate",
        crate::types::AnimDirection::AlternateReverse => "alternate-reverse",
    }
}

fn serialize_inline_animation_fill_mode(fill_mode: &crate::types::FillMode) -> &'static str {
    match fill_mode {
        crate::types::FillMode::None => "none",
        crate::types::FillMode::Forwards => "forwards",
        crate::types::FillMode::Backwards => "backwards",
        crate::types::FillMode::Both => "both",
    }
}

fn serialize_inline_animation_composition(
    composition: &crate::types::AnimationComposition,
) -> &'static str {
    match composition {
        crate::types::AnimationComposition::Replace => "replace",
        crate::types::AnimationComposition::Add => "add",
        crate::types::AnimationComposition::Accumulate => "accumulate",
    }
}

fn serialize_inline_easing(easing: &crate::types::EasingFn) -> String {
    match easing {
        crate::types::EasingFn::Linear => "linear".to_string(),
        crate::types::EasingFn::Ease => "ease".to_string(),
        crate::types::EasingFn::EaseIn => "ease-in".to_string(),
        crate::types::EasingFn::EaseOut => "ease-out".to_string(),
        crate::types::EasingFn::EaseInOut => "ease-in-out".to_string(),
        crate::types::EasingFn::CubicBezier(a, b, c, d) => format!(
            "cubic-bezier({}, {}, {}, {})",
            trim_inline_float(*a),
            trim_inline_float(*b),
            trim_inline_float(*c),
            trim_inline_float(*d)
        ),
        crate::types::EasingFn::StepStart => "step-start".to_string(),
        crate::types::EasingFn::StepEnd => "step-end".to_string(),
        crate::types::EasingFn::Steps(count, position) => {
            format!(
                "steps({}, {})",
                count,
                serialize_inline_step_position(*position)
            )
        }
        crate::types::EasingFn::LinearPoints(points) => {
            let points = points
                .iter()
                .map(|(input, output)| {
                    format!(
                        "{} {}",
                        trim_inline_float(*output),
                        trim_inline_float(*input)
                    )
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("linear({points})")
        }
    }
}

fn serialize_inline_step_position(position: crate::types::StepPosition) -> &'static str {
    match position {
        crate::types::StepPosition::JumpStart => "jump-start",
        crate::types::StepPosition::JumpEnd => "jump-end",
        crate::types::StepPosition::JumpNone => "jump-none",
        crate::types::StepPosition::JumpBoth => "jump-both",
    }
}

fn parse_inline_border_components<'a>(tokens: &[&'a str]) -> (&'a str, &'a str, &'a str) {
    let mut width = None;
    let mut style = None;
    let mut color = None;
    for token in tokens {
        if width.is_none() && is_inline_border_width(token) {
            width = Some(*token);
        } else if style.is_none() && is_inline_border_style(token) {
            style = Some(*token);
        } else if color.is_none() && crate::css::parse_color(token).is_some() {
            color = Some(*token);
        }
    }
    (
        width.unwrap_or("medium"),
        style.unwrap_or("none"),
        color.unwrap_or("currentcolor"),
    )
}

fn is_inline_border_width(token: &str) -> bool {
    matches!(token, "thin" | "medium" | "thick")
        || token == "0"
        || token.ends_with("px")
        || token.ends_with("em")
        || token.ends_with("rem")
        || token.ends_with("pt")
        || token.ends_with("pc")
        || token.ends_with("cm")
        || token.ends_with("mm")
        || token.ends_with("in")
}

fn is_inline_border_style(token: &str) -> bool {
    matches!(
        token,
        "none"
            | "hidden"
            | "dotted"
            | "dashed"
            | "solid"
            | "double"
            | "groove"
            | "ridge"
            | "inset"
            | "outset"
    )
}

// ─────────────────────────────────────────────────────────────────────────────
// The DOM methods that had no home yet.
//
// Everything here is also on `vybe_widgets::dom::Document` with the same name
// and the same shape, because the two are meant to be interchangeable: see
// `_both_browsers_are_whatwg` in `platforms/web`, which calls every one of them
// through the `Browser` alias and so fails to compile the moment they drift.
// ─────────────────────────────────────────────────────────────────────────────

/// `dataset.fooBar` → `data-foo-bar` (HTML §3.2.6.6).
fn dataset_attribute(key: &str) -> String {
    let mut name = String::from("data-");
    for ch in key.chars() {
        if ch.is_ascii_uppercase() {
            name.push('-');
            name.push(ch.to_ascii_lowercase());
        } else {
            name.push(ch);
        }
    }
    name
}

/// `data-foo-bar` → `fooBar`, or `None` for an attribute that is not `data-*`.
fn dataset_key(attribute: &str) -> Option<String> {
    let rest = attribute.strip_prefix("data-")?;
    let mut key = String::new();
    let mut upper = false;
    for ch in rest.chars() {
        if ch == '-' {
            upper = true;
            continue;
        }
        if upper {
            key.extend(ch.to_uppercase());
            upper = false;
        } else {
            key.push(ch);
        }
    }
    Some(key)
}

/// Walk a rendered subtree, gathering what a reader would actually see.
fn inner_text_into(node: &WebCore, out: &mut String) {
    use crate::types::Display;
    if node.tag == "#text" {
        out.push_str(&node.text);
        return;
    }
    if node.tag == "#comment" {
        return;
    }
    // The whole difference from `textContent`: what the cascade hid is not
    // text the user can read, so it contributes nothing.
    if node.style.display == Display::None {
        return;
    }
    let block = node.style.display != Display::Inline;
    if block && !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    for child in &node.children {
        inner_text_into(child, out);
    }
    if block && !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
}

impl Document {
    // ─── DocumentFragment (DOM §4.2.1, §4.5) ───────────────────────────────

    /// `document.createDocumentFragment()` — a detached parent to build in.
    ///
    /// Detached like every other `create*`, so it lands in `pending_nodes` and
    /// the ordinary `append_child` attaches what goes into it.
    pub fn create_document_fragment(&mut self) -> u32 {
        let arena_id = self.arena.create_document_fragment();
        let mut b = WebCore::new("#document-fragment");
        b.node_id = arena_id.0;
        // It never renders: it exists to be emptied into something that does,
        // and if one is ever left in a tree it must not draw.
        apply_property(std::sync::Arc::make_mut(&mut b.style), "display", "none");
        self.pending_nodes.insert(arena_id.0, b);
        self.next_node_id = self.next_node_id.max(arena_id.0 + 1);
        arena_id.0
    }

    /// Whether this node is a `DocumentFragment` — the question `append_child`
    /// and `insert_before` ask before deciding what inserting it means.
    pub fn is_document_fragment(&self, id: u32) -> bool {
        id != 0
            && self.arena.is_alive(NodeId(id))
            && self.arena.get(NodeId(id)).node_type == crate::dom::arena::NodeType::DocumentFragment
    }

    // ─── Node comparison and normalisation (DOM §4.4, §4.5) ─────────────────

    /// `node.normalize()` — drop empty text nodes and merge adjacent ones.
    pub fn normalize(&mut self, id: u32) {
        if id == 0 {
            return;
        }
        let children = self.child_nodes(id);
        for &child in &children {
            self.normalize(child);
        }
        // The run being merged INTO. Reset by any non-text child, because two
        // text nodes with an element between them are not adjacent.
        let mut run: u32 = 0;
        for child in children {
            if !self.is_text_node(child) {
                run = 0;
                continue;
            }
            let data = self.text_data(child);
            if data.is_empty() {
                // Removed outright, and does NOT break the run — so `a`, ``,
                // `b` merges to `ab`.
                self.remove_child(child);
                continue;
            }
            if run == 0 {
                run = child;
                continue;
            }
            let merged = format!("{}{}", self.text_data(run), data);
            self.set_text_data(run, &merged);
            self.remove_child(child);
        }
    }

    /// `node.isEqualNode(other)` — structural equality, not identity.
    ///
    /// Attributes live in a map, so they compare as the SET the spec says they
    /// are, with no order to get wrong.
    pub fn is_equal_node(&self, a: u32, b: u32) -> bool {
        if a == b {
            // Identity settles it — for a NODE. Here 0 is the null id and a
            // freed id names nothing, and nothing is not equal to itself.
            return a != 0 && self.arena.is_alive(NodeId(a));
        }
        if a == 0 || b == 0 {
            return false;
        }
        if !self.arena.is_alive(NodeId(a)) || !self.arena.is_alive(NodeId(b)) {
            return false;
        }
        if self.node_type(a) != self.node_type(b) {
            return false;
        }
        let (Some(x), Some(y)) = (self.arena.try_get(NodeId(a)), self.arena.try_get(NodeId(b)))
        else {
            return false;
        };
        if x.tag != y.tag || x.namespace != y.namespace {
            return false;
        }
        if self.is_element(a) {
            // DOM §4.5 compares attributes as a SET — "each attribute in A
            // has an attribute in B with the same namespace, local name and
            // value". `AttrMap` is an ordered list and its `==` is
            // order-sensitive, which is right for the list and wrong here, so
            // the set comparison is spelled out rather than borrowed.
            let same_attrs = x.attributes.len() == y.attributes.len()
                && x.attributes
                    .iter()
                    .all(|(k, v)| y.attributes.get(k) == Some(v));
            if !same_attrs || x.attribute_ns != y.attribute_ns {
                return false;
            }
        } else if x.text != y.text {
            return false;
        }
        let ca = self.child_nodes(a);
        let cb = self.child_nodes(b);
        ca.len() == cb.len()
            && ca
                .iter()
                .zip(cb.iter())
                .all(|(&p, &q)| self.is_equal_node(p, q))
    }

    /// The root-to-node path, used to put two nodes in tree order.
    fn ancestor_path(&self, id: u32) -> Vec<u32> {
        let mut path = vec![id];
        let mut current = id;
        loop {
            let parent = self.parent_node(current);
            if parent == 0 {
                break;
            }
            path.push(parent);
            current = parent;
        }
        path.reverse();
        path
    }

    /// `node.compareDocumentPosition(other)` — DOM §4.4's bitmask.
    ///
    /// `DISCONNECTED 0x01`, `PRECEDING 0x02`, `FOLLOWING 0x04`,
    /// `CONTAINS 0x08`, `CONTAINED_BY 0x10`, `IMPLEMENTATION_SPECIFIC 0x20`.
    /// Containment always arrives paired with a direction, which is why the
    /// answers below are two bits and not one.
    pub fn compare_document_position(&self, a: u32, b: u32) -> u16 {
        const DISCONNECTED: u16 = 0x01;
        const PRECEDING: u16 = 0x02;
        const FOLLOWING: u16 = 0x04;
        const CONTAINS: u16 = 0x08;
        const CONTAINED_BY: u16 = 0x10;
        const IMPLEMENTATION_SPECIFIC: u16 = 0x20;

        if a == b {
            return 0;
        }
        let pa = self.ancestor_path(a);
        let pb = self.ancestor_path(b);
        if pa.first() != pb.first() {
            // The spec lets an implementation pick the direction for two
            // detached trees, but requires it to be CONSISTENT. Ordering by
            // the node handle is the cheapest thing that is.
            let direction = if a < b { FOLLOWING } else { PRECEDING };
            return DISCONNECTED | IMPLEMENTATION_SPECIFIC | direction;
        }
        let split = pa
            .iter()
            .zip(pb.iter())
            .position(|(x, y)| x != y)
            .unwrap_or_else(|| pa.len().min(pb.len()));
        if split == pa.len() {
            return CONTAINED_BY | FOLLOWING;
        }
        if split == pb.len() {
            return CONTAINS | PRECEDING;
        }
        let siblings = self.child_nodes(pa[split - 1]);
        let ia = siblings.iter().position(|n| *n == pa[split]);
        let ib = siblings.iter().position(|n| *n == pb[split]);
        match (ia, ib) {
            (Some(ia), Some(ib)) if ia < ib => FOLLOWING,
            (Some(_), Some(_)) => PRECEDING,
            _ => DISCONNECTED | IMPLEMENTATION_SPECIFIC | FOLLOWING,
        }
    }

    // ─── The rest of ParentNode / ChildNode (DOM §4.2.6) ────────────────────

    /// Insert before `reference`, or append when there is no reference.
    ///
    /// `insert_before` rejects a reference of 0 rather than treating it as
    /// "at the end", so every caller below has to say which it means.
    fn insert_or_append(&mut self, parent: u32, child: u32, reference: u32) {
        if reference == 0 {
            self.append_child(parent, child);
        } else {
            self.insert_before(parent, child, reference);
        }
    }

    /// `parent.append(...nodes)`.
    pub fn append(&mut self, parent: u32, nodes: &[u32]) {
        for &node in nodes {
            self.append_child(parent, node);
        }
    }

    /// `parent.prepend(...nodes)`.
    ///
    /// Every node goes before the ORIGINAL first child, not before whatever is
    /// first at the time — inserting each before the running first child would
    /// hand the caller its nodes back reversed.
    pub fn prepend(&mut self, parent: u32, nodes: &[u32]) {
        let reference = self.first_child(parent).unwrap_or(0);
        for &node in nodes {
            self.insert_or_append(parent, node, reference);
        }
    }

    /// `node.before(...nodes)`.
    pub fn before(&mut self, id: u32, nodes: &[u32]) {
        let parent = self.parent_node(id);
        if parent == 0 {
            return;
        }
        for &new in nodes {
            self.insert_before(parent, new, id);
        }
    }

    /// `node.after(...nodes)`.
    pub fn after(&mut self, id: u32, nodes: &[u32]) {
        let parent = self.parent_node(id);
        if parent == 0 {
            return;
        }
        let reference = self.next_sibling(id);
        for &new in nodes {
            self.insert_or_append(parent, new, reference);
        }
    }

    /// `node.replaceWith(...nodes)`.
    pub fn replace_with(&mut self, id: u32, nodes: &[u32]) {
        let parent = self.parent_node(id);
        if parent == 0 {
            return;
        }
        for &new in nodes {
            self.insert_before(parent, new, id);
        }
        self.remove_child(id);
    }

    // ─── Serialisation and adjacent insertion (DOM Parsing §3) ─────────────

    /// `element.outerHTML` (getter) — the serialization of the node ITSELF,
    /// where [`inner_html`](Self::inner_html) writes only its children.
    pub fn outer_html(&self, id: u32) -> String {
        if id == 0 {
            return String::new();
        }
        let node = match self
            .find_webcore(id)
            .or_else(|| self.pending_nodes.get(&id))
        {
            Some(n) => n,
            None => return String::new(),
        };
        let mut out = String::new();
        crate::html::serializer::serialize_box(node, &mut out);
        out
    }

    /// `element.insertAdjacentElement(position, element)`.
    ///
    /// Returns the inserted element, or `None` for an unknown position or a
    /// placement with no parent to hold it — the IDL's `null`.
    pub fn insert_adjacent_element(&mut self, id: u32, position: &str, other: u32) -> Option<u32> {
        match position.to_ascii_lowercase().as_str() {
            "beforebegin" => {
                let parent = self.parent_node(id);
                if parent == 0 {
                    return None;
                }
                self.insert_before(parent, other, id);
            }
            "afterbegin" => {
                let reference = self.first_child(id).unwrap_or(0);
                self.insert_or_append(id, other, reference);
            }
            "beforeend" => self.append_child(id, other),
            "afterend" => {
                let parent = self.parent_node(id);
                if parent == 0 {
                    return None;
                }
                let reference = self.next_sibling(id);
                self.insert_or_append(parent, other, reference);
            }
            _ => return None,
        }
        Some(other)
    }

    /// `element.insertAdjacentText(position, data)`.
    pub fn insert_adjacent_text(&mut self, id: u32, position: &str, data: &str) {
        let text = self.create_text_node(data);
        self.insert_adjacent_element(id, position, text);
    }

    // ─── HTMLElement: dataset, innerText, tabIndex, click (HTML §3.2.6) ─────

    /// `element.dataset` — every `data-*` attribute, keyed the way the IDL
    /// names it rather than the way it is spelled in the markup.
    ///
    /// A snapshot, not the live `DOMStringMap`: Rust has nowhere to put an
    /// object whose property writes reach back into the element, so the pair
    /// [`set_dataset`](Self::set_dataset) / [`remove_dataset`](Self::remove_dataset)
    /// carries the write side.
    pub fn dataset(&self, id: u32) -> Vec<(String, String)> {
        if id == 0 || !self.arena.is_alive(NodeId(id)) {
            return Vec::new();
        }
        let mut out: Vec<(String, String)> = self
            .arena
            .get(NodeId(id))
            .attributes
            .iter()
            .filter_map(|(name, value)| dataset_key(name).map(|k| (k, value.clone())))
            .collect();
        // The attributes live in a map, which has no order of its own. Sorting
        // means two reads of one element answer in the same order.
        out.sort();
        out
    }

    /// `element.dataset[key]`.
    pub fn dataset_get(&self, id: u32, key: &str) -> Option<String> {
        self.get_attribute(id, &dataset_attribute(key))
    }

    /// `element.dataset[key] = value`.
    pub fn set_dataset(&mut self, id: u32, key: &str, value: &str) {
        let name = dataset_attribute(key);
        self.set_attribute(id, &name, value);
    }

    /// `delete element.dataset[key]`.
    pub fn remove_dataset(&mut self, id: u32, key: &str) {
        let name = dataset_attribute(key);
        self.remove_attribute(id, &name);
    }

    /// `element.innerText` — the RENDERED text.
    ///
    /// The difference from `textContent` is the cascade: a subtree the styles
    /// hid contributes nothing, and a block boundary becomes a line break.
    /// Reading `textContent` where a page asked for `innerText` is how hidden
    /// markup leaks into a string a user is shown.
    pub fn inner_text(&self, id: u32) -> String {
        let mut out = String::new();
        if let Some(node) = self
            .find_webcore(id)
            .or_else(|| self.pending_nodes.get(&id))
        {
            inner_text_into(node, &mut out);
        }
        out.trim_matches('\n').to_string()
    }

    /// `element.tabIndex`.
    ///
    /// The default is not zero for everything: HTML §6.6.3 gives 0 to what is
    /// focusable without the attribute and −1 to the rest, so a `<div>` and a
    /// `<button>` answer differently with no `tabindex` on either.
    pub fn tab_index(&self, id: u32) -> i32 {
        if let Some(value) = self.get_attribute(id, "tabindex") {
            if let Ok(parsed) = value.trim().parse::<i32>() {
                return parsed;
            }
        }
        if id == 0 || !self.arena.is_alive(NodeId(id)) {
            return -1;
        }
        match self.arena.get(NodeId(id)).tag.as_str() {
            // A link is only in the sequence once it has somewhere to go.
            "a" | "area" => {
                if self.get_attribute(id, "href").is_some() {
                    0
                } else {
                    -1
                }
            }
            "button" | "input" | "select" | "textarea" | "iframe" => 0,
            _ => -1,
        }
    }

    /// `element.tabIndex = index`.
    pub fn set_tab_index(&mut self, id: u32, index: i32) {
        self.set_attribute(id, "tabindex", &index.to_string());
    }

    /// `element.click()` — a synthetic click, dispatched like a real one.
    ///
    /// Both registries are notified, exactly as a real click does it: the
    /// `HtmlEvent` listeners bound to boxes, and the NodeId-keyed
    /// `EventTargetMap` that carries capture and bubbling. Notifying only one
    /// would make a synthetic click reach a different set of handlers from a
    /// user's, which is the one thing `click()` must not do.
    pub fn click(&mut self, id: u32) {
        if id == 0 {
            return;
        }
        let mut dom_event = crate::dom::events::DomEvent::new("click", id);
        self.dispatch_dom_event(&mut dom_event);
    }

    // ─── CSSOM-View (§5) ───────────────────────────────────────────────────

    /// `element.getClientRects()`.
    ///
    /// One rect per box the element generates. A wrapped inline should report
    /// one per line; until the line boxes are addressable from here it reports
    /// the single border box, which is the correct answer for every element
    /// that generates one box and an under-count for the rest.
    pub fn get_client_rects(&self, id: u32) -> Vec<Rect> {
        if let Some(node) = self.get_node(id) {
            if !node.layout.line_cache.is_empty() {
                return node
                    .layout
                    .line_cache
                    .iter()
                    .filter(|line| line.width > 0.0 && line.height > 0.0)
                    .map(|line| {
                        Rect::new(
                            line.x - self.scroll_x,
                            line.y - self.scroll_y,
                            line.width,
                            line.height,
                        )
                    })
                    .collect();
            }
        }
        self.get_bounding_client_rect(id).into_iter().collect()
    }

    /// `element.scrollIntoView()` — bring the element into the nearest
    /// scrollable ancestor, falling back to the viewport.
    pub fn scroll_into_view(&mut self, id: u32) {
        let Some(target) = self.raw_border_rect_document(id) else {
            return;
        };
        let target_style = self.get_computed_style(id).cloned();
        let margin_top = target_style
            .as_ref()
            .map(|style| {
                self.resolve_scroll_length(&style.scroll_margin_top, style, self.viewport_h)
            })
            .unwrap_or(0.0);
        let margin_bottom = target_style
            .as_ref()
            .map(|style| {
                self.resolve_scroll_length(&style.scroll_margin_bottom, style, self.viewport_h)
            })
            .unwrap_or(0.0);
        let margin_left = target_style
            .as_ref()
            .map(|style| {
                self.resolve_scroll_length(&style.scroll_margin_left, style, self.viewport_w)
            })
            .unwrap_or(0.0);
        let margin_right = target_style
            .as_ref()
            .map(|style| {
                self.resolve_scroll_length(&style.scroll_margin_right, style, self.viewport_w)
            })
            .unwrap_or(0.0);
        let mut handled_by_ancestor = false;
        let mut current = self.parent_node(id);
        while current != 0 {
            let current_style = self.get_computed_style(current).cloned();
            let scrollable_y = current_style
                .as_ref()
                .map(|style| matches!(style.overflow_y, Overflow::Scroll | Overflow::Auto))
                .unwrap_or(false)
                && self.element_scroll_height(current) > self.client_height(current);
            let scrollable_x = current_style
                .as_ref()
                .map(|style| matches!(style.overflow_x, Overflow::Scroll | Overflow::Auto))
                .unwrap_or(false)
                && self.element_scroll_width(current) > self.client_width(current);
            if scrollable_x || scrollable_y {
                if let Some(view) = self.raw_padding_rect_document(current) {
                    let current_scroll_x = self.element_scroll_left(current);
                    let current_scroll_y = self.element_scroll_top(current);
                    let (padding_top, padding_right, padding_bottom, padding_left) = current_style
                        .as_ref()
                        .map(|style| {
                            (
                                self.resolve_scroll_length(
                                    &style.scroll_padding_top,
                                    style,
                                    view.h,
                                ),
                                self.resolve_scroll_length(
                                    &style.scroll_padding_bottom,
                                    style,
                                    view.h,
                                ),
                                self.resolve_scroll_length(
                                    &style.scroll_padding_left,
                                    style,
                                    view.w,
                                ),
                                self.resolve_scroll_length(
                                    &style.scroll_padding_right,
                                    style,
                                    view.w,
                                ),
                            )
                        })
                        .map(|(top, bottom, left, right)| (top, right, bottom, left))
                        .unwrap_or((0.0, 0.0, 0.0, 0.0));
                    let visible_top = view.y + current_scroll_y + padding_top;
                    let visible_bottom = view.y + current_scroll_y + view.h - padding_bottom;
                    let target_top = target.y - margin_top;
                    let target_bottom = target.y + target.h + margin_bottom;
                    let mut next_scroll_y = current_scroll_y;
                    if scrollable_y {
                        if target_top < visible_top {
                            next_scroll_y = target_top - view.y - padding_top;
                        } else if target_bottom > visible_bottom {
                            next_scroll_y = target_bottom - view.y - view.h + padding_bottom;
                        }
                    }

                    let visible_left = view.x + current_scroll_x + padding_left;
                    let visible_right = view.x + current_scroll_x + view.w - padding_right;
                    let target_left = target.x - margin_left;
                    let target_right = target.x + target.w + margin_right;
                    let mut next_scroll_x = current_scroll_x;
                    if scrollable_x {
                        if target_left < visible_left {
                            next_scroll_x = target_left - view.x - padding_left;
                        } else if target_right > visible_right {
                            next_scroll_x = target_right - view.x - view.w + padding_right;
                        }
                    }

                    if (next_scroll_x - current_scroll_x).abs() > f32::EPSILON
                        || (next_scroll_y - current_scroll_y).abs() > f32::EPSILON
                    {
                        self.element_scroll_to(current, next_scroll_x, next_scroll_y);
                    }
                    handled_by_ancestor = true;
                }
            }
            current = self.parent_node(current);
        }
        if !handled_by_ancestor {
            self.scroll_x = (target.x - margin_left).max(0.0);
            self.scroll_y = (target.y - margin_top).max(0.0);
        }
    }

    fn resolve_scroll_length(
        &self,
        length: &crate::types::CssLength,
        style: &crate::types::ComputedStyle,
        percent_basis: f32,
    ) -> f32 {
        let font_px = style.font_size.resolve(16.0, 16.0, self.root_font_px());
        length.resolve_vp(
            font_px,
            percent_basis,
            self.root_font_px(),
            self.viewport_w,
            self.viewport_h,
        )
    }

    /// `element.offsetParent` — the nearest POSITIONED ancestor, else the body.
    pub fn offset_parent(&mut self, id: u32) -> Option<u32> {
        let mut current = self.parent_node(id);
        while current != 0 {
            let tag = self.tag_name(current).unwrap_or("").to_string();
            if tag == "body" || tag == "html" {
                return self.body();
            }
            if self.computed_style_property(current, "position") != "static" {
                return Some(current);
            }
            current = self.parent_node(current);
        }
        None
    }

    /// The origin `offsetLeft` / `offsetTop` measure from.
    ///
    /// ⛔ A statically-positioned `body` is NOT subtracted. Every engine
    /// returns the document-relative offset when the offset parent is the body
    /// — a `<div>` at the top of a default page reports `offsetTop` 8, its
    /// distance from the page, not 0, its distance from the body's padding
    /// edge. Subtracting the body put every such element 8px off, which is
    /// most elements on most pages.
    fn offset_origin(&mut self, id: u32) -> (f32, f32) {
        let Some(parent) = self.offset_parent(id) else {
            return (0.0, 0.0);
        };
        if Some(parent) == self.body()
            && self.computed_style_property(parent, "position") == "static"
        {
            return (0.0, 0.0);
        }
        self.raw_padding_rect_document(parent)
            .map(|r| (r.x, r.y))
            .unwrap_or((0.0, 0.0))
    }

    /// `element.offsetTop` — the border box's top edge, relative to
    /// [`offset_parent`](Self::offset_parent) rather than to the viewport.
    pub fn offset_top(&mut self, id: u32) -> f32 {
        let own = self
            .raw_border_rect_document(id)
            .map(|r| r.y)
            .unwrap_or(0.0);
        own - self.offset_origin(id).1
    }

    /// `element.offsetLeft` — see [`offset_top`](Self::offset_top).
    pub fn offset_left(&mut self, id: u32) -> f32 {
        let own = self
            .raw_border_rect_document(id)
            .map(|r| r.x)
            .unwrap_or(0.0);
        own - self.offset_origin(id).0
    }

    // ─── The namespaced accessors that were missing their pair ─────────────

    /// `element.hasAttributeNS(namespace, localName)`.
    pub fn has_attribute_ns(&self, id: u32, namespace: &str, local_name: &str) -> bool {
        self.get_attribute_ns(id, namespace, local_name).is_some()
    }

    /// `element.removeAttributeNS(namespace, localName)`.
    ///
    /// Matched the same way `getAttributeNS` matches — by namespace and LOCAL
    /// name — so `xlink:href` goes and `href` stays.
    pub fn remove_attribute_ns(&mut self, id: u32, namespace: &str, local_name: &str) {
        if id == 0 || !self.arena.is_alive(NodeId(id)) {
            return;
        }
        let want = (!namespace.is_empty()).then_some(namespace);
        let doomed: Vec<String> = self
            .arena
            .get(NodeId(id))
            .attributes
            .keys()
            .filter(|name| {
                let local = match name.split_once(':') {
                    Some((_, local)) => local,
                    None => name.as_str(),
                };
                if local != local_name {
                    return false;
                }
                let have = self
                    .arena
                    .try_get(NodeId(id))
                    .and_then(|n| n.attribute_ns.get(*name));
                have.map(String::as_str) == want
            })
            .cloned()
            .collect();
        for name in doomed {
            self.remove_attribute(id, &name);
        }
    }
}
