use std::ops::DerefMut;

use crate::{
    geometry::{rect::RectF, vector::Vector2F},
    DebugContext, Element, ElementBox, ElementStateHandle, Event, EventContext, LayoutContext,
    MutableAppContext, PaintContext, SizeConstraint,
};
use serde_json::json;

pub struct MouseEventHandler {
    state: ElementStateHandle<MouseState>,
    child: ElementBox,
    click_handler: Option<Box<dyn FnMut(&mut EventContext)>>,
    drag_handler: Option<Box<dyn FnMut(Vector2F, &mut EventContext)>>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct MouseState {
    pub hovered: bool,
    pub clicked: bool,
    prev_drag_position: Option<Vector2F>,
}

impl MouseEventHandler {
    pub fn new<Tag, F, C>(id: usize, cx: &mut C, render_child: F) -> Self
    where
        Tag: 'static,
        F: FnOnce(&MouseState, &mut C) -> ElementBox,
        C: DerefMut<Target = MutableAppContext>,
    {
        let state_handle = cx.element_state::<Tag, _>(id);
        let child = state_handle.update(cx, |state, cx| render_child(state, cx));
        Self {
            state: state_handle,
            child,
            click_handler: None,
            drag_handler: None,
        }
    }

    pub fn on_click(mut self, handler: impl FnMut(&mut EventContext) + 'static) -> Self {
        self.click_handler = Some(Box::new(handler));
        self
    }

    pub fn on_drag(mut self, handler: impl FnMut(Vector2F, &mut EventContext) + 'static) -> Self {
        self.drag_handler = Some(Box::new(handler));
        self
    }
}

impl Element for MouseEventHandler {
    type LayoutState = ();
    type PaintState = ();

    fn layout(
        &mut self,
        constraint: SizeConstraint,
        cx: &mut LayoutContext,
    ) -> (Vector2F, Self::LayoutState) {
        (self.child.layout(constraint, cx), ())
    }

    fn paint(
        &mut self,
        bounds: RectF,
        _: &mut Self::LayoutState,
        cx: &mut PaintContext,
    ) -> Self::PaintState {
        self.child.paint(bounds.origin(), cx);
    }

    fn dispatch_event(
        &mut self,
        event: &Event,
        bounds: RectF,
        _: &mut Self::LayoutState,
        _: &mut Self::PaintState,
        cx: &mut EventContext,
    ) -> bool {
        let click_handler = self.click_handler.as_mut();
        let drag_handler = self.drag_handler.as_mut();

        let handled_in_child = self.child.dispatch_event(event, cx);

        self.state.update(cx, |state, cx| match event {
            Event::MouseMoved { position } => {
                let mouse_in = bounds.contains_point(*position);
                if state.hovered != mouse_in {
                    state.hovered = mouse_in;
                    cx.notify();
                    true
                } else {
                    handled_in_child
                }
            }
            Event::LeftMouseDown { position, .. } => {
                if !handled_in_child && bounds.contains_point(*position) {
                    state.clicked = true;
                    state.prev_drag_position = Some(*position);
                    cx.notify();
                    true
                } else {
                    handled_in_child
                }
            }
            Event::LeftMouseUp { position, .. } => {
                state.prev_drag_position = None;
                if !handled_in_child && state.clicked {
                    state.clicked = false;
                    cx.notify();
                    if let Some(handler) = click_handler {
                        if bounds.contains_point(*position) {
                            handler(cx);
                        }
                    }
                    true
                } else {
                    handled_in_child
                }
            }
            Event::LeftMouseDragged { position, .. } => {
                if !handled_in_child && state.clicked {
                    let prev_drag_position = state.prev_drag_position.replace(*position);
                    if let Some((handler, prev_position)) = drag_handler.zip(prev_drag_position) {
                        let delta = *position - prev_position;
                        if !delta.is_zero() {
                            (handler)(delta, cx);
                        }
                    }
                    true
                } else {
                    handled_in_child
                }
            }
            _ => handled_in_child,
        })
    }

    fn debug(
        &self,
        _: RectF,
        _: &Self::LayoutState,
        _: &Self::PaintState,
        cx: &DebugContext,
    ) -> serde_json::Value {
        json!({
            "type": "MouseEventHandler",
            "child": self.child.debug(cx),
        })
    }
}
