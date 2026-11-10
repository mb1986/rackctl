//! Following cables through patch panels.

use super::{Cabling, End, PatchSide, SocketId};

impl Cabling {
    /// Returns the endpoints along the cable path through `socket`, from one end to the
    /// other: starting at `socket`, or for a patch-panel port, at the far end of its back.
    /// A path that comes back to one of its endpoints stops there.
    #[must_use]
    pub fn path(&self, socket: SocketId) -> Vec<SocketId> {
        let mut path = vec![socket];
        if !self.is_patch_port(socket) {
            self.walk(End { socket, side: None }, &mut path);
            return path;
        }
        self.walk(End { socket, side: Some(PatchSide::Back) }, &mut path);
        path.reverse();
        self.walk(End { socket, side: Some(PatchSide::Front) }, &mut path);
        path
    }

    /// Follows the cables from `end`, adding each endpoint reached to `path`, and through
    /// each patch port to its other side.
    fn walk(&self, mut end: End, path: &mut Vec<SocketId>) {
        while let Some(next) = self.connected(end) {
            if path.contains(&next.socket) {
                break;
            }
            path.push(next.socket);
            let Some(side) = next.side else { break };
            end = End { socket: next.socket, side: Some(side.opposite()) };
        }
    }
}

#[cfg(test)]
mod tests {
    use indoc::indoc;

    use crate::catalog::{Catalog, Part};
    use crate::rack::{DeviceId, Layout, Rack};
    use crate::testing::dir_with;
    use crate::wiring::resolve::tests::resolve;
    use crate::wiring::{Wiring, endpoint_name};

    use super::*;

    const WIRING: &str = indoc! {"
        power pdu:8 srv01:psu1
        mgmt srv01:mgmt patch:b-f15 sw:10
        net sw:XG1 patch:f-b12 patch:b-f16 srv02:nic1
        net router:0 patch:b3
        net patch:b20 patch:f21
        net patch:b21 patch:f20"};

    /// Returns the path through the endpoint `from`, written the short way.
    fn path(from: &str) -> Vec<String> {
        let (_, layout, result) = resolve(WIRING);
        let cabling = result.expect("valid wiring");
        let catalog = Catalog::builtin();
        let socket = cabling.find(from, &layout, &catalog).expect("endpoint");
        let name = |socket| endpoint_name(cabling.endpoint(socket), &layout, &catalog);
        cabling.path(socket).into_iter().map(name).collect()
    }

    #[test]
    fn follows_a_path_through_patch_ports() {
        let mgmt = ["srv01:mgmt", "patch:15", "sw:10"];
        assert_eq!(path("srv01:mgmt"), mgmt);
        assert_eq!(path("sw:10"), ["sw:10", "patch:15", "srv01:mgmt"]);
        // From a patch port, the path starts at the far end of its back.
        for from in ["patch:15", "patch:b15", "patch:front15", "patch:port15"] {
            assert_eq!(path(from), mgmt, "{from}");
        }
        assert_eq!(path("pdu:8"), ["pdu:8", "srv01:psu1"]);
    }

    #[test]
    fn follows_a_jumper_between_patch_ports() {
        let jumper = ["sw:XG1", "patch:12", "patch:16", "srv02:nic1"];
        assert_eq!(path("sw:XG1"), jumper);
        assert_eq!(path("patch:12"), ["srv02:nic1", "patch:16", "patch:12", "sw:XG1"]);
    }

    #[test]
    fn stops_where_the_cables_end() {
        assert_eq!(path("router:0"), ["router:0", "patch:3"]);
        assert_eq!(path("patch:3"), ["router:0", "patch:3"]);
        assert_eq!(path("sw:20"), ["sw:20"]);
        // Two patch ports cabled to each other's back and front make a loop.
        assert_eq!(path("patch:20"), ["patch:21", "patch:20"]);
    }

    #[test]
    fn finds_what_feeds_a_device() {
        let (_, layout, result) = resolve(WIRING);
        let cabling = result.expect("valid wiring");
        let catalog = Catalog::builtin();
        let srv01 = layout.devices().position(|device| device.id == "srv01").expect("srv01");
        let srv01 = DeviceId(srv01);
        let feeds: Vec<String> = cabling
            .sockets(srv01, Part::Psu)
            .filter_map(|socket| cabling.connected(End { socket, side: None }))
            .map(|end| endpoint_name(cabling.endpoint(end.socket), &layout, &catalog))
            .collect();
        assert_eq!(feeds, ["pdu:8"]);
        assert_eq!(cabling.sockets(srv01, Part::Psu).count(), 2);
    }

    /// Asserts that the name of every endpoint of `layout` finds that endpoint again.
    #[track_caller]
    fn assert_names_find_their_endpoints(layout: &Layout, catalog: &Catalog, cabling: &Cabling) {
        for device in (0..layout.devices().count()).map(DeviceId) {
            for part in Part::ENDPOINTS {
                for socket in cabling.sockets(device, part) {
                    let name = endpoint_name(cabling.endpoint(socket), layout, catalog);
                    let found =
                        cabling.find(&name, layout, catalog).map_err(|p| p.message().to_owned());
                    assert_eq!(found, Ok(socket), "{name}");
                }
            }
        }
    }

    #[test]
    fn finds_an_endpoint_written_with_leading_zeros() {
        let (_, layout, result) = resolve(WIRING);
        let cabling = result.expect("valid wiring");
        let catalog = Catalog::builtin();
        let find = |text| cabling.find(text, &layout, &catalog).expect(text);
        assert_eq!(find("pdu:08"), find("pdu:8"));
        assert_eq!(find("srv01:psu01"), find("srv01:psu1"));
    }

    #[test]
    fn names_every_endpoint_so_that_it_is_found_again() {
        let (_, layout, result) = resolve(WIRING);
        assert_names_find_their_endpoints(&layout, &Catalog::builtin(), &result.expect("wiring"));
    }

    #[test]
    fn names_endpoints_with_their_rack_when_there_are_several() {
        let rack = |name| {
            let text = format!(
                r#"rack "{name}" units=2 {{
                    device "sw" model="cisco/sg350x-48p" u=2
                    device "patch" model="generic/patchpanel-24" u=1
                }}"#
            );
            Rack::parse(&text).expect("valid rack")
        };
        let layout = Layout::new(vec![rack("lab"), rack("office")]);
        let catalog = Catalog::builtin();
        let wiring = Wiring::parse(
            "wiring { net lab.sw:1 lab.patch:f-b1 office.patch:b-f1 office.sw:XG1; }",
        );
        let cabling = wiring.expect("wiring").resolve(&layout, &catalog).expect("valid wiring");

        let from = cabling.find("office.sw:XG1", &layout, &catalog).expect("endpoint");
        let name = |socket| endpoint_name(cabling.endpoint(socket), &layout, &catalog);
        let path: Vec<_> = cabling.path(from).into_iter().map(name).collect();
        assert_eq!(path, ["office.sw:XG1", "office.patch:1", "lab.patch:1", "lab.sw:1"]);
        assert_names_find_their_endpoints(&layout, &catalog, &cabling);
    }

    #[test]
    fn names_a_grouped_port_on_any_kind_of_device() {
        let kvm = r##"model { name "KVM"; kind "kvm"; ports 6
            face #"""
            nnnn gg
            ~
            """#
            legend { n port; g port group="XG"; ~ fill } }"##;
        let dir = dir_with(&[("x/kvm.kdl", kvm)]);
        let mut catalog = Catalog::builtin();
        catalog.add_dirs(&[dir.path()]).expect("readable catalog");
        let rack = Rack::parse(r#"rack "r" units=1 { device "kvm" model="x/kvm" u=1 }"#)
            .expect("valid rack");
        let layout = Layout::new(vec![rack]);
        let cabling = Wiring::parse("wiring {}").expect("wiring").resolve(&layout, &catalog);
        let cabling = cabling.expect("valid wiring");
        let xg1 = cabling.sockets(DeviceId(0), Part::Port).nth(4).expect("port XG1");
        assert_eq!(endpoint_name(cabling.endpoint(xg1), &layout, &catalog), "kvm:XG1");
        assert_names_find_their_endpoints(&layout, &catalog, &cabling);
    }
}
