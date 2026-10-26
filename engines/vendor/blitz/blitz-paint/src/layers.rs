// Modified by Wega contributors, 2026: preserve CSS compositing blend modes.
use anyrender::{Filter, PaintScene};
use kurbo::{Affine, Shape};
use peniko::{Fill, Mix};
use std::{cell::Cell, sync::Arc};

const LAYER_LIMIT: u32 = 1024;

#[derive(Default)]
pub(crate) struct LayerManager {
    layers_used: Cell<u32>,
    layer_depth: Cell<u32>,
    layers_wanted: Cell<u32>,

    #[allow(unused)] // Only used for debugging. Enabled as required.
    layer_depth_used: Cell<u32>,
}

impl LayerManager {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn with_blend_layer<S: PaintScene, F: FnOnce(&mut S)>(
        &self, scene: &mut S, condition: bool, blend: Mix, opacity: f32,
        transform: Affine, shape: &impl Shape, filter: Option<Arc<Filter>>,
        backdrop_filter: Option<Arc<Filter>>, paint: F,
    ) {
        let used = self.maybe_push_blend_layer(scene, condition, blend, opacity, transform, shape, filter, backdrop_filter);
        paint(scene);
        self.maybe_pop_layer(scene, used);
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn maybe_with_layer<S: PaintScene, F: FnOnce(&mut S)>(
        &self,
        scene: &mut S,
        condition: bool,
        opacity: f32,
        transform: Affine,
        shape: &impl Shape,
        filter: Option<Arc<Filter>>,
        backdrop_filter: Option<Arc<Filter>>,
        paint_layer: F,
    ) {
        let layer_used = self.maybe_push_layer(
            scene,
            condition,
            opacity,
            transform,
            shape,
            filter,
            backdrop_filter,
        );
        paint_layer(scene);
        self.maybe_pop_layer(scene, layer_used);
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn maybe_push_layer(
        &self,
        scene: &mut impl PaintScene,
        condition: bool,
        opacity: f32,
        transform: Affine,
        shape: &impl Shape,
        filter: Option<Arc<Filter>>,
        backdrop_filter: Option<Arc<Filter>>,
    ) -> bool {
        self.maybe_push_blend_layer(scene, condition, Mix::Normal, opacity, transform, shape, filter, backdrop_filter)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn maybe_push_blend_layer(
        &self,
        scene: &mut impl PaintScene,
        condition: bool,
        blend: Mix,
        opacity: f32,
        transform: Affine,
        shape: &impl Shape,
        filter: Option<Arc<Filter>>,
        backdrop_filter: Option<Arc<Filter>>,
    ) -> bool {
        if !condition {
            return false;
        }
        self.layers_wanted.update(|x| x + 1);

        // Check if clips are above limit
        let layers_available = self.layers_used.get() <= LAYER_LIMIT;
        if !layers_available {
            return false;
        }

        // Actually push the layer
        if blend == Mix::Normal && opacity == 1.0 && filter.is_none() && backdrop_filter.is_none() {
            scene.push_clip_layer(Fill::NonZero, transform, shape);
        } else {
            scene.push_layer(
                Fill::NonZero,
                blend,
                opacity,
                transform,
                shape,
                filter,
                backdrop_filter,
            );
        };

        // Update accounting
        self.layers_used.update(|x| x + 1);
        self.layer_depth.update(|x| x + 1);
        self.layer_depth.update(|x| x.max(self.layer_depth.get()));

        true
    }

    pub(crate) fn maybe_pop_layer(&self, scene: &mut impl PaintScene, condition: bool) {
        if condition {
            scene.pop_layer();
            self.layer_depth.update(|x| x - 1);
        }
    }
}
