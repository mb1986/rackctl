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
    use crate::rack::Rack;
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
        let (_, rack, result) = resolve(WIRING);
        let cabling = result.expect("valid wiring");
        let catalog = Catalog::builtin();
        let socket = cabling.find(from, &rack, &catalog).expect("endpoint");
        let name = |socket| endpoint_name(cabling.endpoint(socket), &rack, &catalog);
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
        let (_, rack, result) = resolve(WIRING);
        let cabling = result.expect("valid wiring");
        let catalog = Catalog::builtin();
        let srv01 = rack.devices.iter().position(|device| device.id == "srv01").expect("srv01");
        let feeds: Vec<String> = cabling
            .sockets(srv01, Part::Psu)
            .filter_map(|socket| cabling.connected(End { socket, side: None }))
            .map(|end| endpoint_name(cabling.endpoint(end.socket), &rack, &catalog))
            .collect();
        assert_eq!(feeds, ["pdu:8"]);
        assert_eq!(cabling.sockets(srv01, Part::Psu).count(), 2);
    }

    /// Asserts that the name of every endpoint of `rack` finds that endpoint again.
    #[track_caller]
    fn assert_names_find_their_endpoints(rack: &Rack, catalog: &Catalog, cabling: &Cabling) {
        for device in 0..rack.devices.len() {
            for part in Part::ENDPOINTS {
                for socket in cabling.sockets(device, part) {
                    let name = endpoint_name(cabling.endpoint(socket), rack, catalog);
                    let found =
                        cabling.find(&name, rack, catalog).map_err(|p| p.message().to_owned());
                    assert_eq!(found, Ok(socket), "{name}");
                }
            }
        }
    }

    #[test]
    fn finds_an_endpoint_written_with_leading_zeros() {
        let (_, rack, result) = resolve(WIRING);
        let cabling = result.expect("valid wiring");
        let catalog = Catalog::builtin();
        let find = |text| cabling.find(text, &rack, &catalog).expect(text);
        assert_eq!(find("pdu:08"), find("pdu:8"));
        assert_eq!(find("srv01:psu01"), find("srv01:psu1"));
    }

    #[test]
    fn names_every_endpoint_so_that_it_is_found_again() {
        let (_, rack, result) = resolve(WIRING);
        assert_names_find_their_endpoints(&rack, &Catalog::builtin(), &result.expect("wiring"));
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
        let cabling = Wiring::parse("wiring {}").expect("wiring").resolve(&rack, &catalog);
        let cabling = cabling.expect("valid wiring");
        let xg1 = cabling.sockets(0, Part::Port).nth(4).expect("port XG1");
        assert_eq!(endpoint_name(cabling.endpoint(xg1), &rack, &catalog), "kvm:XG1");
        assert_names_find_their_endpoints(&rack, &catalog, &cabling);
    }
}
