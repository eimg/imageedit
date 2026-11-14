//! Explicit movement toggles and directional shadow attachments.
use super::{DocumentContent, LayerId};
use crate::preview::{Dirty, PixelRect};

impl DocumentContent {
    // Keep the historical wire shape, with at most one movement set. A single
    // enabled toggle is meaningful and survives until the user turns it off.
    pub fn layer_links(&self) -> &[Vec<LayerId>] {
        &self.layer_links
    }

    pub fn linked_layers(&self, id: LayerId) -> &[LayerId] {
        self.layer_links
            .first()
            .filter(|set| set.contains(&id))
            .map_or(&[], Vec::as_slice)
    }

    pub fn set_layer_links(&mut self, groups: Vec<Vec<LayerId>>) -> anyhow::Result<()> {
        let mut seen = std::collections::HashSet::new();
        anyhow::ensure!(
            groups.iter().all(|group| !group.is_empty()
                && group
                    .iter()
                    .all(|id| self.layer(*id).is_some() && seen.insert(*id))),
            "Invalid layer links"
        );
        // Older files can have several groups. Their enabled members become
        // the one explicit movement set; never infer attachments from names.
        let mut joined: Vec<_> = groups.into_iter().flatten().collect();
        joined.sort_unstable_by_key(|id| id.0);
        self.layer_links = if joined.is_empty() {
            vec![]
        } else {
            vec![joined]
        };
        Ok(())
    }

    #[cfg(test)]
    pub fn link_layers(&mut self, first: LayerId, second: LayerId) -> Dirty {
        if first == second || self.layer(first).is_none() || self.layer(second).is_none() {
            return Dirty::None;
        }
        let a = self.set_layer_linked(first, true);
        let b = self.set_layer_linked(second, true);
        if a == Dirty::None && b == Dirty::None {
            Dirty::None
        } else {
            Dirty::Unpainted
        }
    }

    pub fn set_layer_linked(&mut self, id: LayerId, linked: bool) -> Dirty {
        if self.layer(id).is_none() || (!self.linked_layers(id).is_empty()) == linked {
            return Dirty::None;
        }
        if !linked {
            return self.unlink_layer(id);
        }
        if self.layer_links.is_empty() {
            self.layer_links.push(Vec::new());
        }
        self.layer_links[0].push(id);
        self.layer_links[0].sort_unstable_by_key(|id| id.0);
        Dirty::Unpainted
    }

    pub fn unlink_layer(&mut self, id: LayerId) -> Dirty {
        let Some(set) = self.layer_links.first_mut().filter(|set| set.contains(&id)) else {
            return Dirty::None;
        };
        set.retain(|member| *member != id);
        self.layer_links.retain(|set| !set.is_empty());
        Dirty::Unpainted
    }

    pub fn shadow_attachments(&self) -> &[(LayerId, LayerId)] {
        &self.shadow_attachments
    }

    pub fn shadow_source(&self, shadow: LayerId) -> Option<LayerId> {
        self.shadow_attachments
            .iter()
            .find(|(child, _)| *child == shadow)
            .map(|(_, parent)| *parent)
    }

    pub fn set_shadow_attachments(
        &mut self,
        attachments: Vec<(LayerId, LayerId)>,
    ) -> anyhow::Result<()> {
        let mut children = std::collections::HashSet::new();
        anyhow::ensure!(
            attachments.len() < self.layers.len()
                && attachments.iter().all(|(child, parent)| child != parent
                    && self.layer(*child).is_some()
                    && self.layer(*parent).is_some()
                    && children.insert(*child)),
            "Invalid shadow attachments"
        );
        for &(child, mut parent) in &attachments {
            let mut remaining = attachments.len();
            loop {
                anyhow::ensure!(parent != child && remaining > 0, "Cyclic shadow attachment");
                let Some((_, next)) = attachments.iter().find(|(id, _)| *id == parent) else {
                    break;
                };
                parent = *next;
                remaining -= 1;
            }
        }
        self.shadow_attachments = attachments;
        Ok(())
    }

    pub fn detach_shadow(&mut self, id: LayerId) -> Dirty {
        let before = self.shadow_attachments.len();
        self.shadow_attachments.retain(|(child, _)| *child != id);
        if before == self.shadow_attachments.len() {
            Dirty::None
        } else {
            Dirty::Unpainted
        }
    }

    pub(super) fn prune_layer_links(&mut self) {
        let layers = &self.layers;
        for set in &mut self.layer_links {
            set.retain(|id| layers.iter().any(|layer| layer.id == *id));
        }
        self.layer_links.retain(|set| !set.is_empty());
        self.shadow_attachments.retain(|(child, parent)| {
            layers.iter().any(|layer| layer.id == *child)
                && layers.iter().any(|layer| layer.id == *parent)
        });
    }

    // Start with explicitly linked members only when the moved layer is linked.
    // Then follow attachments downward. Moving a shadow never pulls its source.
    pub fn movement_layers(&self, id: LayerId) -> Vec<LayerId> {
        if self.layer(id).is_none() {
            return vec![];
        }
        let mut members = match self.linked_layers(id) {
            [] => vec![id],
            set => set.to_vec(),
        };
        let mut index = 0;
        while index < members.len() {
            let parent = members[index];
            for &(child, source) in &self.shadow_attachments {
                if source == parent && !members.contains(&child) {
                    members.push(child);
                }
            }
            index += 1;
        }
        members
    }

    pub fn move_blocker(&self, id: LayerId) -> Option<LayerId> {
        self.movement_layers(id).into_iter().find(|member| {
            self.layer(*member)
                .is_some_and(|layer| layer.locks.position)
        })
    }

    pub fn can_move_linked(&self, id: LayerId) -> bool {
        self.layer(id).is_some() && self.move_blocker(id).is_none()
    }

    // Validate the whole translation before moving anything, including hidden
    // members and attached descendants. No overflow may publish a partial move.
    pub fn can_move_linked_to(&self, id: LayerId, offset: (i32, i32)) -> bool {
        let Some(layer) = self.layer(id) else {
            return false;
        };
        let delta = (
            i64::from(offset.0) - i64::from(layer.offset.0),
            i64::from(offset.1) - i64::from(layer.offset.1),
        );
        self.can_move_linked(id)
            && self.movement_layers(id).iter().all(|member| {
                self.layer(*member).is_some_and(|layer| {
                    i32::try_from(i64::from(layer.offset.0) + delta.0).is_ok()
                        && i32::try_from(i64::from(layer.offset.1) + delta.1).is_ok()
                })
            })
    }

    pub fn move_linked_layers(&mut self, id: LayerId, offset: (i32, i32)) -> Dirty {
        if !self.can_move_linked_to(id, offset) {
            return Dirty::None;
        }
        let origin = self.layer(id).unwrap().offset;
        if origin == offset {
            return Dirty::None;
        }
        let delta = (
            i64::from(offset.0) - i64::from(origin.0),
            i64::from(offset.1) - i64::from(origin.1),
        );
        let members = self.movement_layers(id);
        let mut bounds: Option<PixelRect> = None;
        for member in members {
            let origin = self.layer(member).unwrap().offset;
            let target = (
                (i64::from(origin.0) + delta.0) as i32,
                (i64::from(origin.1) + delta.1) as i32,
            );
            if let Dirty::Composite(rect) = self.set_offset(member, target) {
                bounds = Some(bounds.map_or(rect, |old| {
                    let left = old.x.min(rect.x);
                    let top = old.y.min(rect.y);
                    PixelRect::new(
                        left,
                        top,
                        (old.right().max(rect.right()) - left) as u32,
                        (old.bottom().max(rect.bottom()) - top) as u32,
                    )
                }));
            }
        }
        bounds.map_or(Dirty::Unpainted, Dirty::Composite)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        document::{CropRect, LayerLocks},
        filters::FilterSpec,
        geometry::Alignment,
        transform::Resample,
    };
    use image::{Rgba, RgbaImage};
    use std::sync::Arc;

    fn scene() -> DocumentContent {
        DocumentContent::single_layer(
            "Source".into(),
            RgbaImage::from_pixel(8, 8, Rgba([200, 80, 40, 255])),
        )
    }

    fn shadow_spec() -> FilterSpec {
        FilterSpec::Shadow {
            radius: 1,
            offset: (2, 3),
            opacity: 180,
            color: Rgba([0, 0, 0, 255]),
        }
    }

    #[test]
    fn shadows_follow_sources_without_enabling_links_and_move_independently() {
        let mut content = scene();
        let source = content.top_layer_id();
        content.place_drop_shadow(source, shadow_spec()).unwrap();
        content.place_drop_shadow(source, shadow_spec()).unwrap();
        let shadow = content.layers()[0].id;
        assert!(content.layer_links().is_empty());
        assert_eq!(content.shadow_source(shadow), Some(source));
        let baseline = content.clone();
        let origin = content.layer(shadow).unwrap().offset;
        content.move_linked_layers(shadow, (origin.0 - 3, origin.1 + 2));
        assert_eq!(content.layer(source).unwrap().offset, (0, 0));
        assert_eq!(
            content.layer(content.layers()[1].id).unwrap().offset,
            baseline.layers()[1].offset
        );
        let adjusted = content.layer(shadow).unwrap().offset;
        content.move_linked_layers(source, (4, 1));
        for layer in content.layers() {
            let old = baseline.layer(layer.id).unwrap();
            let before = if layer.id == shadow {
                adjusted
            } else {
                old.offset
            };
            assert_eq!(layer.offset, (before.0 + 4, before.1 + 1));
            assert!(Arc::ptr_eq(&layer.pixels, &old.pixels));
        }
        content.align_layer(source, Alignment::Right);
        assert_eq!(content.layer(source).unwrap().offset.0, 0);
        assert_eq!(content.detach_shadow(shadow), Dirty::Unpainted);
        let detached = content.layer(shadow).unwrap().offset;
        content.move_linked_layers(source, (-2, -3));
        assert_eq!(content.layer(shadow).unwrap().offset, detached);
        assert_eq!(content.shadow_source(shadow), None);
    }

    #[test]
    fn manual_toggles_form_one_set_and_attachments_do_not_change_membership() {
        let mut content = scene();
        let a = content.top_layer_id();
        let b = content.add_layer(a, "B".into(), Rgba([20; 4])).unwrap();
        let c = content.add_layer(b, "C".into(), Rgba([30; 4])).unwrap();
        assert_eq!(content.set_layer_linked(a, true), Dirty::Unpainted);
        assert_eq!(content.linked_layers(a), &[a]);
        assert_eq!(content.set_layer_linked(a, true), Dirty::None);
        content.set_layer_linked(b, true);
        content.place_drop_shadow(a, shadow_spec()).unwrap();
        let shadow = content.shadow_attachments()[0].0;
        assert!(content.linked_layers(shadow).is_empty());
        let offset = content.layer(shadow).unwrap().offset;
        content.move_linked_layers(b, (3, -2));
        assert_eq!(content.layer(a).unwrap().offset, (3, -2));
        assert_eq!(
            content.layer(shadow).unwrap().offset,
            (offset.0 + 3, offset.1 - 2)
        );
        assert_eq!(content.layer(c).unwrap().offset, (0, 0));
        content.move_linked_layers(c, (7, 7));
        assert_eq!(content.layer(a).unwrap().offset, (3, -2));
        content.unlink_layer(a);
        assert_eq!(content.linked_layers(b), &[b]);
        content.move_linked_layers(b, (5, 5));
        assert_eq!(content.layer(a).unwrap().offset, (3, -2));
        content.move_linked_layers(a, (4, -1));
        assert_eq!(
            content.layer(shadow).unwrap().offset,
            (offset.0 + 4, offset.1 - 1)
        );
        // Explicitly linking the shadow too still translates it only once.
        content.set_layer_linked(a, true);
        content.set_layer_linked(shadow, true);
        content.move_linked_layers(a, (5, 0));
        assert_eq!(
            content.layer(shadow).unwrap().offset,
            (offset.0 + 5, offset.1)
        );
    }

    #[test]
    fn attached_descendants_block_atomically_and_are_pruned_on_delete_or_merge() {
        let mut content = scene();
        let source = content.top_layer_id();
        content.place_drop_shadow(source, shadow_spec()).unwrap();
        let shadow = content.shadow_attachments()[0].0;
        content.place_drop_shadow(shadow, shadow_spec()).unwrap();
        let child = content.shadow_attachments()[1].0;
        let shadow_offset = content.layer(shadow).unwrap().offset;
        content.set_visible(child, false);
        content.set_layer_locks(
            child,
            LayerLocks {
                position: true,
                alpha: false,
            },
        );
        assert_eq!(content.move_blocker(source), Some(child));
        assert_eq!(content.move_linked_layers(source, (2, 2)), Dirty::None);
        assert_eq!(content.layer(source).unwrap().offset, (0, 0));
        content.set_layer_locks(child, LayerLocks::default());
        content.set_offset(child, (i32::MAX, 0));
        assert_eq!(content.move_linked_layers(source, (1, 1)), Dirty::None);
        assert_eq!(content.layer(shadow).unwrap().offset, shadow_offset);
        content.set_offset(child, (0, 0));
        assert_ne!(content.move_linked_layers(source, (2, 2)), Dirty::None);
        assert_eq!(content.layer(child).unwrap().offset, (2, 2));
        content.delete_layer(source).unwrap();
        assert_eq!(content.shadow_source(shadow), None);
        assert_eq!(content.shadow_source(child), Some(shadow));
        content.set_visible(child, true);
        content.merge_down(shadow).unwrap();
        assert!(content.shadow_attachments().is_empty());
    }

    #[test]
    fn hidden_members_follow_but_any_position_lock_or_overflow_blocks_the_entire_move() {
        let mut content = scene();
        let source = content.top_layer_id();
        let peer = content
            .add_layer(source, "Hidden".into(), Rgba([0; 4]))
            .unwrap();
        content.set_offset(peer, (2, -3));
        content.link_layers(source, peer);
        content.set_visible(peer, false);
        content.move_linked_layers(source, (5, 6));
        assert_eq!(content.layer(peer).unwrap().offset, (7, 3));
        content.set_layer_locks(
            peer,
            LayerLocks {
                alpha: false,
                position: true,
            },
        );
        assert!(!content.can_move_linked(source));
        assert_eq!(content.move_linked_layers(source, (8, 9)), Dirty::None);
        assert_eq!(content.layer(source).unwrap().offset, (5, 6));
        assert_eq!(content.layer(peer).unwrap().offset, (7, 3));
        content.set_layer_locks(peer, LayerLocks::default());
        assert_eq!(
            content.move_linked_layers(source, (i32::MAX, 6)),
            Dirty::None
        );
        assert_eq!(content.layer(peer).unwrap().offset, (7, 3));
        content.set_layer_locks(
            peer,
            LayerLocks {
                alpha: true,
                position: false,
            },
        );
        assert!(content.can_move_linked(source));
        assert_ne!(content.move_linked_layers(source, (-5, -6)), Dirty::None);
    }

    #[test]
    fn duplicate_and_layer_transforms_stay_independent_and_deleted_or_clipped_links_are_pruned() {
        let mut content = scene();
        let a = content.top_layer_id();
        let b = content.add_layer(a, "Peer".into(), Rgba([20; 4])).unwrap();
        let c = content.add_layer(b, "Third".into(), Rgba([30; 4])).unwrap();
        content.link_layers(a, b);
        content.link_layers(b, c);
        let copy = content.duplicate_layer(a).unwrap();
        assert!(content.linked_layers(copy).is_empty());
        assert!(content.scale_layer(a, 4, 4, Resample::Nearest));
        content.set_offset(a, (1, 1));
        assert_eq!(content.layer(b).unwrap().offset, (0, 0));
        assert_eq!(content.layer(b).unwrap().pixels.dimensions(), (8, 8));
        content.delete_layer(b).unwrap();
        assert_eq!(content.linked_layers(a), &[a, c]);
        content.set_offset(c, (100, 100));
        assert!(content.crop(CropRect {
            x: 0,
            y: 0,
            width: 4,
            height: 4
        }));
        assert!(content.layer(c).is_none());
        assert_eq!(content.linked_layers(a), &[a]);
    }

    #[test]
    fn merging_detaches_the_result_and_unlinking_a_pair_keeps_the_other_toggle() {
        let mut content = scene();
        let a = content.top_layer_id();
        let b = content.add_layer(a, "Peer".into(), Rgba([20; 4])).unwrap();
        content.link_layers(a, b);
        content.unlink_layer(b);
        assert_eq!(content.linked_layers(a), &[a]);
        content.link_layers(a, b);
        assert_eq!(content.merge_down(b), Some(a));
        assert!(content.layer_links().is_empty());
    }
}
