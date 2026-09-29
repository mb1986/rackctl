//! Checked wiring: the cables between the sockets of the rack's devices.

use super::{LinkKind, PatchSide};
use crate::catalog::Part;

/// One endpoint of one device, such as PSU 1 of srv01, as its place in [`Cabling`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SocketId(u32);

impl SocketId {
    const fn at(self) -> usize {
        self.0 as usize
    }
}

/// Which device and endpoint a socket is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Endpoint {
    /// The device's position in the rack's devices.
    pub device: usize,
    pub part: Part,
    /// The endpoint's place among those of its part, as [`Model::endpoint_index`] gives it.
    ///
    /// [`Model::endpoint_index`]: crate::catalog::Model::endpoint_index
    pub index: u16,
}

/// One end of a cable: a socket, and for a patch-panel port, the side it is plugged into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct End {
    pub socket: SocketId,
    pub side: Option<PatchSide>,
}

impl End {
    /// Returns the end's place in a table of every socket's two slots.
    pub(super) fn place(self) -> usize {
        self.socket.at() * 2 + slot(self.side)
    }
}

/// A cable between two sockets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Link {
    pub kind: LinkKind,
    pub ends: [End; 2],
}

/// The cables between the rack's devices, with every endpoint looked up and every use
/// checked. Each endpoint of each device is a socket in one table, so following a cable is
/// a lookup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cabling {
    links: Vec<Link>,
    sockets: Vec<Socket>,
    /// Where the sockets of each device's parts start, by device and then part in
    /// [`Part::ENDPOINTS`] order, followed by the end of the table.
    starts: Vec<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Socket {
    endpoint: Endpoint,
    /// The cable plugged in, as a position in `links`: a patch port's back and front, or
    /// the first slot for any other endpoint.
    links: [Option<u32>; 2],
}

impl Cabling {
    /// Returns cabling without cables for devices with `counts` endpoints of each part, in
    /// [`Part::ENDPOINTS`] order.
    pub(super) fn new(counts: &[[u16; Part::ENDPOINTS.len()]]) -> Self {
        let mut starts = Vec::with_capacity(counts.len() * Part::ENDPOINTS.len() + 1);
        let mut sockets = Vec::new();
        for (device, counts) in counts.iter().enumerate() {
            for (&part, &count) in Part::ENDPOINTS.iter().zip(counts) {
                starts.push(position(sockets.len()));
                sockets.extend((0..count).map(|index| Socket {
                    endpoint: Endpoint { device, part, index },
                    links: [None; 2],
                }));
            }
        }
        starts.push(position(sockets.len()));
        Self { links: Vec::new(), sockets, starts }
    }

    /// Returns the number of places for an [`End`]: two slots for each socket.
    pub(super) const fn places(&self) -> usize {
        self.sockets.len() * 2
    }

    /// Returns the cables in the order they are written.
    #[must_use]
    pub fn links(&self) -> &[Link] {
        &self.links
    }

    /// Returns the socket of an endpoint, if the device has it.
    #[must_use]
    pub fn socket(&self, device: usize, part: Part, index: u16) -> Option<SocketId> {
        let part = Part::ENDPOINTS.iter().position(|&endpoint| endpoint == part)?;
        let at = device * Part::ENDPOINTS.len() + part;
        let (start, end) = (*self.starts.get(at)?, *self.starts.get(at + 1)?);
        let id = start + u32::from(index);
        (id < end).then_some(SocketId(id))
    }

    /// Returns which device and endpoint a socket is.
    #[must_use]
    pub fn endpoint(&self, socket: SocketId) -> Endpoint {
        self.sockets[socket.at()].endpoint
    }

    /// Returns the cable plugged in at `end`, if there is one.
    #[must_use]
    pub fn link_at(&self, end: End) -> Option<&Link> {
        let link = self.sockets[end.socket.at()].links[slot(end.side)]?;
        self.links.get(link as usize)
    }

    /// Adds a cable whose ends are free.
    pub(super) fn connect(&mut self, link: Link) {
        let id = position(self.links.len());
        for end in link.ends {
            self.sockets[end.socket.at()].links[slot(end.side)] = Some(id);
        }
        self.links.push(link);
    }
}

/// Returns the slot a cable at `side` takes in a socket.
fn slot(side: Option<PatchSide>) -> usize {
    usize::from(side == Some(PatchSide::Front))
}

/// Converts a position in a table to the width stored for it.
fn position(at: usize) -> u32 {
    u32::try_from(at).unwrap_or(u32::MAX)
}
