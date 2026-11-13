//! One retained effect per raster layer. Pixels cache the displayed result;
//! the source and local coverage allow replacing its parameters without a
//! second destructive application. Subsequent pixel/geometry edits bake it.
use crate::{
    adjustments::Adjustment,
    document::{DocumentContent, LayerId, MAX_STACK_BYTES},
    filters::FilterSpec,
    preview::Dirty,
    selection::Selection,
};
use image::{Rgba, RgbaImage};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EffectSpec {
    Adjustment(Adjustment),
    Blur { radius: u32 },
    Sharpen { radius: u32, amount: u8 },
}
impl EffectSpec {
    pub fn valid(self) -> bool {
        match self {
            Self::Adjustment(value) => value.valid(),
            Self::Blur { radius } | Self::Sharpen { radius, .. } => {
                radius <= crate::filters::MAX_RADIUS
            }
        }
    }
    pub fn identity(self) -> bool {
        match self {
            Self::Adjustment(value) => value.is_identity(),
            Self::Blur { radius } => radius == 0,
            Self::Sharpen { radius, amount } => radius == 0 || amount == 0,
        }
    }
    pub fn filter(self) -> Option<FilterSpec> {
        match self {
            Self::Adjustment(_) => None,
            Self::Blur { radius } => Some(FilterSpec::Blur { radius }),
            Self::Sharpen { radius, amount } => Some(FilterSpec::Sharpen { radius, amount }),
        }
    }
}
#[derive(Clone)]
pub struct EditableEffect {
    pub spec: EffectSpec,
    pub source: Arc<RgbaImage>,
    pub coverage: Option<Arc<RgbaImage>>,
    pub alpha_locked: bool,
}
impl EditableEffect {
    pub fn storage(&self) -> impl Iterator<Item = &Arc<RgbaImage>> {
        std::iter::once(&self.source).chain(self.coverage.iter())
    }
}
impl DocumentContent {
    pub fn can_retain_effect(&self, id: LayerId, selection: &Selection, reedit: bool) -> bool {
        let Some(layer) = self.layer(id).filter(|layer| layer.text.is_none()) else {
            return false;
        };
        if reedit {
            return layer.effect.is_some();
        }
        let used: u64 = self
            .layers()
            .iter()
            .flat_map(|layer| layer.storage())
            .map(|buffer| buffer.as_raw().len() as u64)
            .sum();
        let previous: u64 = layer
            .effect
            .iter()
            .flat_map(|effect| effect.storage())
            .map(|buffer| buffer.as_raw().len() as u64)
            .sum();
        let added = layer.pixels.as_raw().len() as u64
            * if *selection == Selection::Unrestricted {
                1
            } else {
                2
            };
        used.saturating_sub(previous).saturating_add(added) <= MAX_STACK_BYTES
    }
    pub fn retained_effect_input(&self, id: LayerId) -> Option<(Self, Selection)> {
        let layer = self.layer(id)?;
        let effect = layer.effect.as_ref()?;
        let selection = effect
            .coverage
            .as_ref()
            .map_or(Selection::Unrestricted, |coverage| {
                let (width, height) = self.size();
                Selection::from_coverage(width, height, |x, y| {
                    let (x, y) = (
                        i64::from(x) - i64::from(layer.offset.0),
                        i64::from(y) - i64::from(layer.offset.1),
                    );
                    if x < 0
                        || y < 0
                        || x >= i64::from(coverage.width())
                        || y >= i64::from(coverage.height())
                    {
                        0
                    } else {
                        coverage.get_pixel(x as u32, y as u32)[3]
                    }
                })
            });
        let mut content = self.clone();
        let target = content.layers.iter_mut().find(|layer| layer.id == id)?;
        target.pixels = effect.source.clone();
        target.effect = None;
        target.locks.alpha = effect.alpha_locked;
        Some((content, selection))
    }
    pub fn apply_retained_effect(
        &mut self,
        id: LayerId,
        spec: EffectSpec,
        selection: &Selection,
        reedit: bool,
    ) -> Dirty {
        if !spec.valid()
            || !self.can_retain_effect(id, selection, reedit)
            || (!reedit && spec.identity())
        {
            return Dirty::None;
        }
        let Some(index) = self.layer_index(id) else {
            return Dirty::None;
        };
        let previous = self.layers[index].clone();
        if reedit
            && previous
                .effect
                .as_ref()
                .is_some_and(|effect| effect.spec == spec)
        {
            return Dirty::None;
        }
        let effect = if reedit {
            previous.effect.clone().unwrap()
        } else {
            let offset = previous.offset;
            let (width, height) = previous.pixels.dimensions();
            let canvas = self.size();
            EditableEffect {
                spec,
                source: previous.pixels.clone(),
                alpha_locked: previous.locks.alpha,
                coverage: (*selection != Selection::Unrestricted).then(|| {
                    Arc::new(RgbaImage::from_fn(width, height, |x, y| {
                        Rgba([
                            255,
                            255,
                            255,
                            selection.coverage(
                                i64::from(offset.0) + i64::from(x),
                                i64::from(offset.1) + i64::from(y),
                                canvas.0,
                                canvas.1,
                            ),
                        ])
                    }))
                }),
            }
        };
        let source = &effect.source;
        let coverage = |x: u32, y: u32| {
            if effect.alpha_locked && source.get_pixel(x, y)[3] == 0 {
                0
            } else {
                effect
                    .coverage
                    .as_ref()
                    .map_or(255, |coverage| coverage.get_pixel(x, y)[3])
            }
        };
        let result = if spec.identity() {
            source.clone()
        } else {
            let adjusted = match spec {
                EffectSpec::Adjustment(value) => {
                    crate::adjustments::apply_pixels(source, value, coverage, |pixel| {
                        self.mode().constrain(pixel)
                    })
                    .map(|adjusted| adjusted.pixels)
                }
                _ => {
                    let filtered = match spec {
                        EffectSpec::Blur { radius } => {
                            crate::filters::gaussian_blur_alpha(source, radius, effect.alpha_locked)
                        }
                        EffectSpec::Sharpen { radius, amount } => crate::filters::sharpen_alpha(
                            source,
                            radius,
                            amount,
                            effect.alpha_locked,
                        ),
                        _ => unreachable!(),
                    };
                    crate::filters::apply_covered(source, &filtered, coverage, |pixel| {
                        self.mode().constrain(pixel)
                    })
                    .map(|adjusted| adjusted.pixels)
                }
            };
            adjusted.map_or_else(|| source.clone(), Arc::new)
        };
        self.layers[index].pixels = result;
        self.layers[index].effect = None;
        let unchanged_pixels = self.layers[index].pixels == previous.pixels;
        if unchanged_pixels && !spec.identity() && !reedit {
            self.layers[index] = previous;
            return Dirty::None;
        }
        if !spec.identity() {
            self.layers[index].effect = Some(EditableEffect { spec, ..effect });
        }
        if unchanged_pixels {
            Dirty::Unpainted
        } else {
            self.layer_pixels_dirty(id)
        }
    }
    pub fn remove_retained_effect(&mut self, id: LayerId, bake: bool) -> Dirty {
        let Some(index) = self.layer_index(id) else {
            return Dirty::None;
        };
        let Some(effect) = self.layers[index].effect.take() else {
            return Dirty::None;
        };
        if bake {
            Dirty::Unpainted
        } else {
            self.layers[index].pixels = effect.source;
            self.layer_pixels_dirty(id)
        }
    }
}
