//! Disjoint movement groups. Pixel edits and layer transforms remain independent.
use super::{DocumentContent, LayerId};
use crate::preview::{Dirty, PixelRect};

impl DocumentContent {
    pub fn layer_links(&self) -> &[Vec<LayerId>] {
        &self.layer_links
    }

    pub fn linked_layers(&self, id: LayerId) -> &[LayerId] {
        self.layer_links
            .iter()
            .find(|group| group.contains(&id))
            .map_or(&[], Vec::as_slice)
    }

    pub fn set_layer_links(&mut self, groups: Vec<Vec<LayerId>>) -> anyhow::Result<()> {
        let mut seen = std::collections::HashSet::new();
        anyhow::ensure!(
            groups.len() <= self.layers.len() / 2
                && groups.iter().all(|group| {
                    (2..=self.layers.len()).contains(&group.len())
                        && group
                            .iter()
                            .all(|id| self.layer(*id).is_some() && seen.insert(*id))
                }),
            "Invalid layer links"
        );
        self.layer_links = groups;
        Ok(())
    }

    pub fn link_layers(&mut self, first: LayerId, second: LayerId) -> Dirty {
        if first == second || self.layer(first).is_none() || self.layer(second).is_none() {
            return Dirty::None;
        }
        if self.linked_layers(first).contains(&second) {
            return Dirty::None;
        }
        let mut joined = vec![first, second];
        self.layer_links.retain(|group| {
            if group.contains(&first) || group.contains(&second) {
                joined.extend(group);
                false
            } else {
                true
            }
        });
        joined.sort_unstable_by_key(|id| id.0);
        joined.dedup();
        self.layer_links.push(joined);
        Dirty::Unpainted
    }

    pub fn unlink_layer(&mut self, id: LayerId) -> Dirty {
        let Some(group) = self
            .layer_links
            .iter_mut()
            .find(|group| group.contains(&id))
        else {
            return Dirty::None;
        };
        group.retain(|member| *member != id);
        self.layer_links.retain(|group| group.len() >= 2);
        Dirty::Unpainted
    }

    pub(super) fn prune_layer_links(&mut self) {
        let layers = &self.layers;
        for group in &mut self.layer_links {
            group.retain(|id| layers.iter().any(|layer| layer.id == *id));
        }
        self.layer_links.retain(|group| group.len() >= 2);
    }

    pub fn can_move_linked(&self, id: LayerId) -> bool {
        self.layer(id).is_some_and(|layer| !layer.locks.position)
            && self.linked_layers(id).iter().all(|member| {
                self.layer(*member)
                    .is_some_and(|layer| !layer.locks.position)
            })
    }

    // Validate the whole translation before moving anything, including hidden members.
    pub fn can_move_linked_to(&self, id: LayerId, offset: (i32, i32)) -> bool {
        let Some(layer) = self.layer(id) else {
            return false;
        };
        let delta = (
            i64::from(offset.0) - i64::from(layer.offset.0),
            i64::from(offset.1) - i64::from(layer.offset.1),
        );
        self.can_move_linked(id)
            && self.linked_layers(id).iter().all(|member| {
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
        let members = match self.linked_layers(id) {
            [] => vec![id],
            group => group.to_vec(),
        };
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

    #[test]
    fn new_and_repeated_shadows_link_to_source_and_move_from_either_member() {
        let mut content = scene();
        let source = content.top_layer_id();
        let spec = FilterSpec::Shadow {
            radius: 1,
            offset: (2, 3),
            opacity: 180,
            color: Rgba([0, 0, 0, 255]),
        };
        content.place_drop_shadow(source, spec).unwrap();
        content.place_drop_shadow(source, spec).unwrap();
        let shadow = content.layers()[0].id;
        assert_eq!(content.linked_layers(source).len(), 3);
        let baseline = content.clone();
        let origin = content.layer(shadow).unwrap().offset;
        assert_ne!(
            content.move_linked_layers(shadow, (origin.0 - 3, origin.1 + 2)),
            Dirty::None
        );
        for layer in content.layers() {
            let old = baseline.layer(layer.id).unwrap();
            assert_eq!(layer.offset, (old.offset.0 - 3, old.offset.1 + 2));
            assert!(Arc::ptr_eq(&layer.pixels, &old.pixels));
        }
        content.align_layer(source, Alignment::Right);
        assert_eq!(content.layer(source).unwrap().offset.0, 0);
        assert_eq!(content.layer(shadow).unwrap().offset.0, origin.0);
        assert_eq!(content.unlink_layer(shadow), Dirty::Unpainted);
        let detached = content.layer(shadow).unwrap().offset;
        content.move_linked_layers(source, (4, 1));
        assert_eq!(content.layer(shadow).unwrap().offset, detached);
        assert_eq!(content.linked_layers(source).len(), 2);
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
        assert!(content.layer_links().is_empty());
    }

    #[test]
    fn merging_detaches_the_result_and_unlinking_a_pair_clears_both_indicators() {
        let mut content = scene();
        let a = content.top_layer_id();
        let b = content.add_layer(a, "Peer".into(), Rgba([20; 4])).unwrap();
        content.link_layers(a, b);
        content.unlink_layer(b);
        assert!(content.layer_links().is_empty());
        content.link_layers(a, b);
        assert_eq!(content.merge_down(b), Some(a));
        assert!(content.layer_links().is_empty());
    }
}
