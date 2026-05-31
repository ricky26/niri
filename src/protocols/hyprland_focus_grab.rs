use std::collections::{HashMap, HashSet};

use hyprland_focus_grab_manager_v1::HyprlandFocusGrabManagerV1;
use hyprland_focus_grab_v1::HyprlandFocusGrabV1;
use smithay::reexports::wayland_server::{
    Client, DataInit, Dispatch, DisplayHandle, GlobalDispatch, New, Resource,
};
use smithay::wayland::{Dispatch2, GlobalDispatch2};
use wayland_server::backend::ClientId;
use wayland_server::protocol::wl_surface::WlSurface;

use crate::protocols::EmptyData;
use super::raw::hyprland_focus_grab_v1::v1::server::hyprland_focus_grab_manager_v1;
use super::raw::hyprland_focus_grab_v1::v1::server::hyprland_focus_grab_v1;

const VERSION: u32 = 1;

pub struct HyprlandFocusGrabState {
    pub surfaces: HashSet<WlSurface>,
    pub pending_surfaces: HashSet<WlSurface>,
}

pub struct HyprlandFocusGrabManagerState {
    pub grabs: HashMap<HyprlandFocusGrabV1, HyprlandFocusGrabState>,
    pub active_grab: Option<HyprlandFocusGrabV1>,
    pub focus: Option<WlSurface>,
}

pub struct HyprlandFocusGrabManagerGlobalData {
    filter: Box<dyn for<'c> Fn(&'c Client) -> bool + Send + Sync>,
}

pub trait HyprlandFocusGrabHandler {
    fn hyprland_focus_grab_manager_state(&mut self) -> &mut HyprlandFocusGrabManagerState;

    fn ensure_focus(&mut self, surfaces: &HashSet<WlSurface>);
}

impl HyprlandFocusGrabManagerState {
    pub fn new<D, F>(display: &DisplayHandle, filter: F) -> Self
    where
        D: GlobalDispatch<HyprlandFocusGrabManagerV1, HyprlandFocusGrabManagerGlobalData>,
        D: HyprlandFocusGrabHandler,
        D: 'static,
        F: for<'c> Fn(&'c Client) -> bool + Send + Sync + 'static,
    {
        let global_data = HyprlandFocusGrabManagerGlobalData {
            filter: Box::new(filter),
        };
        display.create_global::<D, HyprlandFocusGrabManagerV1, _>(VERSION, global_data);

        Self {
            grabs: HashMap::new(),
            active_grab: None,
            focus: None,
        }
    }

    pub fn has_grab(&self) -> bool {
        self.active_grab.is_some()
    }

    pub fn clear(&mut self) -> bool {
        if let Some(grab) = self.active_grab.take() {
            grab.cleared();
            self.focus = None;
            true
        } else {
            false
        }
    }

    pub fn try_update_focus(&mut self, focus: &WlSurface) -> bool {
        let Some(grab) = &self.active_grab else {
            return false;
        };

        let Some(data) = self.grabs.get_mut(grab) else {
            return false;
        };

        if !data.surfaces.contains(focus) {
            self.clear();
            return false;
        }

        self.focus = Some(focus.clone());
        true
    }
}

impl<D> GlobalDispatch2<HyprlandFocusGrabManagerV1, D>
    for HyprlandFocusGrabManagerGlobalData
where
    D: Dispatch<HyprlandFocusGrabManagerV1, EmptyData>,
    D: HyprlandFocusGrabHandler,
{
    fn bind(
        &self,
        _state: &mut D,
        _handle: &DisplayHandle,
        _client: &Client,
        manager: New<HyprlandFocusGrabManagerV1>,
        data_init: &mut DataInit<'_, D>,
    ) {
        data_init.init(manager, EmptyData);
    }

    fn can_view(&self, client: &Client) -> bool {
        (self.filter)(&client)
    }
}

impl<D> Dispatch2<HyprlandFocusGrabManagerV1, D> for EmptyData
where
    D: Dispatch<HyprlandFocusGrabV1, EmptyData>,
    D: HyprlandFocusGrabHandler,
{
    fn request(
        &self,
        handler: &mut D,
        _client: &Client,
        _resource: &HyprlandFocusGrabManagerV1,
        request: <HyprlandFocusGrabManagerV1 as Resource>::Request,
        _dhandle: &DisplayHandle,
        data_init: &mut DataInit<'_, D>,
    ) {
        match request {
            hyprland_focus_grab_manager_v1::Request::CreateGrab { grab } => {
                let state = handler.hyprland_focus_grab_manager_state();
                let grab = data_init.init(grab, EmptyData);
                let grab_state = HyprlandFocusGrabState {
                    surfaces: HashSet::new(),
                    pending_surfaces: HashSet::new(),
                };
                state.grabs.insert(grab, grab_state);
            }
            hyprland_focus_grab_manager_v1::Request::Destroy => {}
        }
    }
}

impl<D> Dispatch2<HyprlandFocusGrabV1, D> for EmptyData
where
    D: HyprlandFocusGrabHandler,
{
    fn request(
        &self,
        handler: &mut D,
        client: &Client,
        resource: &HyprlandFocusGrabV1,
        request: <HyprlandFocusGrabV1 as Resource>::Request,
        _dhandle: &DisplayHandle,
        _data_init: &mut DataInit<'_, D>,
    ) {
        let state = handler.hyprland_focus_grab_manager_state();
        let Some(grab_state) = state.grabs.get_mut(resource) else {
            warn!("request sent to destroyed grab");
            return;
        };

        match request {
            hyprland_focus_grab_v1::Request::AddSurface { surface } => {
                grab_state.pending_surfaces.insert(surface);
            }
            hyprland_focus_grab_v1::Request::RemoveSurface { surface } => {
                grab_state.pending_surfaces.remove(&surface);
            }
            hyprland_focus_grab_v1::Request::Commit => {
                let new_surfaces = grab_state.pending_surfaces.clone();
                grab_state.surfaces = new_surfaces;
                if !grab_state.surfaces.is_empty() {
                    let focus = grab_state.surfaces.iter().next().unwrap().clone();
                    state.clear();
                    state.active_grab = Some(resource.clone());
                    state.focus = Some(focus);
                }
            }
            hyprland_focus_grab_v1::Request::Destroy => {
                self.destroyed(handler, client.id(), resource);
            }
        }
    }

    fn destroyed(
        &self,
        handler: &mut D,
        _client: ClientId,
        resource: &HyprlandFocusGrabV1,
    ) {
        let state = handler.hyprland_focus_grab_manager_state();
        if state.active_grab.as_ref() == Some(resource) {
            state.clear();
        }
        state.grabs.remove(resource);
    }
}

