# Relay mode across machines, in NixOS VMs. `server` runs soulfind; `controller` runs reseek
# through services.reseek behind the module's nginx, whose certificate the module orders from
# pebble on `acme`; `agent` runs reseek-agent through services.reseek-agent and dials the
# controller by its https name; `peer` is a second reseek that browses and downloads the agent's
# files from the controller's user. The script drives each daemon through its HTTP API.
{
  self,
  pkgs,
  soulfind,
}:
let
  inherit (pkgs) lib;

  # the controller's public name, which its certificate is issued for
  host = "reseek.test";
  soulfindPort = 2242;
  listenPort = 50300;
  users = {
    controller = "relayhost";
    peer = "relaypeer";
  };
  secrets = {
    controllerSoulseek = "controller-soulfind-pw-4c1e";
    controllerAdmin = "controller-admin-pw-90ab";
    controllerKey = "controller-admin-key-2f7d1c";
    agentKey = "agent-readwrite-key-8e3b5a";
    agentSecret = "nas-relay-secret-61f0d4";
    wrongSecret = "not-the-nas-secret-0a9c";
    peerSoulseek = "peer-soulfind-pw-73d2";
    peerAdmin = "peer-admin-pw-e5c8";
    peerKey = "peer-admin-key-b42a91";
  };

  envFile =
    name: values:
    pkgs.writeText name (lib.concatLines (lib.mapAttrsToList (key: value: "${key}=${value}") values));

  # a daemon logged in to soulfind as `username`, reachable by other users on its listen port and
  # by the script on its HTTP port 5030
  daemon = username: {
    enable = true;
    openFirewall = true;
    settings = {
      soulseek = {
        inherit username;
        address = "server";
        port = soulfindPort;
        listen_port = listenPort;
        port_mapping = false;
      };
      flags.no_version_check = true;
      web.https.disabled = true;
    };
  };

  # the files the agent shares, by path under /srv/music, with their sizes: a nested directory,
  # a name with spaces and non-ASCII characters, and files large enough to be in flight for a
  # while under the peer's speed limit
  agentFiles = {
    "Album/01 small.flac" = 200000;
    "Album/Disc 2/02 large.flac" = 32 * 1024 * 1024;
    "Ünïcödé Ärtist/03 späces & ñ.flac" = 300000;
    "Album/04 cut.flac" = 64 * 1024 * 1024;
    "Album/05 restart.flac" = 32 * 1024 * 1024;
  };

  # a oneshot that writes `files` of random bytes under `root` before `unit` starts
  shareFiles = root: unit: files: {
    wantedBy = [ unit ];
    before = [ unit ];
    serviceConfig = {
      Type = "oneshot";
      RemainAfterExit = true;
    };
    script = lib.concatLines (
      lib.mapAttrsToList (name: size: ''
        mkdir -p "$(dirname ${lib.escapeShellArg "${root}/${name}"})"
        head -c ${toString size} /dev/urandom > ${lib.escapeShellArg "${root}/${name}"}
      '') files
    );
  };

  # another reseek-agent beside the module's, with the module's settings and `overrides` over them,
  # and the secrets of `env`; started only by the script
  otherAgent = config: name: overrides: env: {
    serviceConfig = {
      ExecStart = lib.escapeShellArgs [
        (lib.getExe' config.services.reseek-agent.package "reseek-agent")
        "--config"
        ((pkgs.formats.toml { }).generate "${name}.toml" (
          lib.recursiveUpdate (lib.filterAttrsRecursive (
            _: value: value != null
          ) config.services.reseek-agent.settings) overrides
        ))
        "--app-dir"
        "/var/lib/${name}"
      ];
      EnvironmentFile = envFile "${name}.env" env;
      DynamicUser = true;
      StateDirectory = name;
    };
  };
in
pkgs.testers.runNixOSTest {
  name = "reseek-relay";

  nodes = {
    acme = {
      imports = [ "${pkgs.path}/nixos/tests/common/acme/server" ];
    };

    server = {
      systemd.services.soulfind = {
        wantedBy = [ "multi-user.target" ];
        serviceConfig = {
          ExecStart = "${lib.getExe soulfind} -p ${toString soulfindPort} -d /var/lib/soulfind/soulfind.db";
          DynamicUser = true;
          StateDirectory = "soulfind";
        };
      };
      networking.firewall.allowedTCPPorts = [ soulfindPort ];
    };

    controller = {
      imports = [
        self.nixosModules.default
        "${pkgs.path}/nixos/tests/common/acme/client"
      ];
      services.reseek = lib.recursiveUpdate (daemon users.controller) {
        environmentFile = envFile "controller.env" {
          RESEEK_SLSK_PASSWORD = secrets.controllerSoulseek;
          RESEEK_PASSWORD = secrets.controllerAdmin;
          RESEEK_API_KEY = secrets.controllerKey;
        };
        extraConfigFiles = [
          ((pkgs.formats.toml { }).generate "relay-secrets.toml" {
            web.authentication.api_keys.agents = {
              key = secrets.agentKey;
              role = "readwrite";
            };
            relay.agents.nas = {
              instance_name = "nas";
              secret = secrets.agentSecret;
            };
          })
        ];
        settings.relay = {
          enabled = true;
          mode = "controller";
        };
        reverseProxy = { inherit host; };
      };
      # http for the ACME challenge, https for the agent
      networking.firewall.allowedTCPPorts = [
        80
        443
      ];
      networking.hosts."127.0.0.1" = [ host ];
    };

    agent =
      { config, nodes, ... }:
      {
        imports = [ self.nixosModules.default ];
        services.reseek-agent = {
          enable = true;
          environmentFile = envFile "agent.env" {
            RESEEK_CONTROLLER_API_KEY = secrets.agentKey;
            RESEEK_CONTROLLER_SECRET = secrets.agentSecret;
          };
          settings = {
            instance_name = "nas";
            relay.controller = {
              address = "https://${host}";
              downloads = true;
            };
            shares.directories = [ "/srv/music" ];
          };
        };
        systemd.services.share-files = shareFiles "/srv/music" "reseek-agent.service" agentFiles;
        # the agent `nas` with a secret the controller does not hold
        systemd.services.wrong-secret = otherAgent config "wrong-secret" { } {
          RESEEK_CONTROLLER_API_KEY = secrets.agentKey;
          RESEEK_CONTROLLER_SECRET = secrets.wrongSecret;
        };
        # an agent whose system store is the stock bundle, without the test CA, and with the wrong
        # secret so it could not take the place of the module's
        systemd.services.untrusting = otherAgent config "untrusting" { } {
          RESEEK_CONTROLLER_API_KEY = secrets.agentKey;
          RESEEK_CONTROLLER_SECRET = secrets.wrongSecret;
          SSL_CERT_FILE = "${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt";
        };
        networking.hosts.${nodes.controller.networking.primaryIPAddress} = [ host ];
      };

    peer = {
      imports = [ self.nixosModules.default ];
      services.reseek = lib.recursiveUpdate (daemon users.peer) {
        environmentFile = envFile "peer.env" {
          RESEEK_SLSK_PASSWORD = secrets.peerSoulseek;
          RESEEK_PASSWORD = secrets.peerAdmin;
          RESEEK_API_KEY = secrets.peerKey;
        };
        settings = {
          shares.directories = [ "/srv/gifts" ];
          # in KiB/s, so a large file is in flight for several seconds
          transfers.download.speed_limit = 2048;
        };
      };
      systemd.services.share-files = shareFiles "/srv/gifts" "reseek.service" {
        "gift.flac" = 150000;
      };
    };
  };

  testScript = ''
    import datetime
    import json
    import shlex
    import urllib.parse
    from typing import Any, Callable

    keys = {"controller": "${secrets.controllerKey}", "peer": "${secrets.peerKey}"}
    users = {"controller": "${users.controller}", "peer": "${users.peer}"}
    agent_files = ${builtins.toJSON agentFiles}
    secrets = ${builtins.toJSON (lib.attrValues secrets)}


    def api(node: Any, path: str, method: str = "GET", body: Any = None) -> Any:
        """The answer of the daemon on `node` to `method` on the API route `path`."""
        command = (
            f"curl -sS --fail-with-body -X {method} -H 'x-api-key: {keys[node.name]}'"
            f" http://127.0.0.1:5030/api/v0/{path}"
        )
        if body is not None:
            command += " -H 'content-type: application/json'"
            command += f" --data-binary {shlex.quote(json.dumps(body))}"
        text = node.succeed(command)
        return json.loads(text) if text.strip() else None


    def until(
        node: Any,
        what: str,
        met: Callable[[Any], bool],
        path: str = "application",
        seconds: int = 120,
    ) -> Any:
        """The answer to `path` on `node` once `met` holds for it, polled each second."""
        seen: list[Any] = [None]

        def check(_last: bool) -> bool:
            seen[0] = api(node, path)
            return met(seen[0])

        try:
            retry(check, timeout=datetime.timedelta(seconds=seconds))
        except Exception as e:
            raise Exception(
                f"{node.name}: {what} did not happen; last answer: {json.dumps(seen[0])}"
            ) from e
        return seen[0]


    def user(name: str) -> str:
        return urllib.parse.quote(name, safe="")


    def serving(state: Any, files: int) -> bool:
        """Whether the controller serves the agent nas with `files` files."""
        agents = state["relay"]["agents"]
        return (
            [a["name"] for a in agents] == ["nas"]
            and agents[0]["ready"]
            and state["shares"]["files"] == files
        )


    def flattened(groups: list[Any]) -> list[Any]:
        """The transfers of a listing grouped by user and directory."""
        return [f for g in groups for d in g["directories"] for f in d["files"]]


    def download(node: Any, username: str, files: list[tuple[str, int]]) -> list[str]:
        """Asks `node` to download `files`, each a remote path and size, from `username`; their ids."""
        job = {
            "groups": [
                {
                    "username": username,
                    "files": [{"filename": f, "size": s} for f, s in files],
                }
            ]
        }
        answer = api(node, "transfers/downloads/jobs", "POST", job)
        group = answer["groups"][0]
        assert group["error"] is None and group["failures"] == [], f"not enqueued: {group}"
        return [t["id"] for t in group["enqueued"]]


    def finished(node: Any, username: str, download_id: str, seconds: int = 120) -> Any:
        """The download `download_id` from `username` once it succeeded."""
        return until(
            node,
            f"the download {download_id} succeeding",
            lambda t: t["state"] == "Completed, Succeeded",
            f"transfers/downloads/{user(username)}/{download_id}",
            seconds,
        )


    def sha256(node: Any, path: str) -> str:
        return node.succeed(f"sha256sum {shlex.quote(path)}").split()[0]


    def remote(path: str) -> str:
        """The name the controller's user shares the agent's file `path` under."""
        return "music\\" + path.replace("/", "\\")


    def main_pid(node: Any, unit: str) -> str:
        return node.succeed(f"systemctl show -P MainPID {unit}").strip()


    start_all()

    with subtest("the servers are up and the controller's certificate comes from the ACME server"):
        # soulfind exits when a connection comes from loopback on a machine with no default route,
        # so its port is not probed from its own machine
        server.wait_for_unit("soulfind.service")
        acme.wait_for_unit("pebble.service")
        controller.wait_for_unit("reseek.service")
        controller.wait_for_unit("nginx.service")
        # the authority pebble issues from is made when it starts, so it is fetched, not built in
        controller.wait_until_succeeds(
            "curl -sSf https://acme.test:15000/roots/0 > /tmp/pebble.pem"
        )
        controller.wait_until_succeeds(
            "curl -sSf --cacert /tmp/pebble.pem https://${host}/health", timeout=180
        )
        until(controller, "the controller logged in", lambda s: s["server"]["isLoggedIn"])
        peer.wait_for_unit("reseek.service")
        peer.wait_for_open_port(5030)
        until(peer, "the peer logged in", lambda s: s["server"]["isLoggedIn"])
        # the agent's machine trusts the test CA as update-ca-certificates would make it: in the
        # system bundle
        pem = controller.succeed("cat /tmp/pebble.pem")
        agent.succeed(
            f"printf %s {shlex.quote(pem)} > /tmp/pebble.pem"
            " && cat /etc/ssl/certs/ca-certificates.crt /tmp/pebble.pem > /tmp/bundle.crt"
            " && rm /etc/ssl/certs/ca-certificates.crt"
            " && mv /tmp/bundle.crt /etc/ssl/certs/ca-certificates.crt"
        )

    with subtest("the agent runs as its own user and logs in through nginx over TLS"):
        agent.wait_for_unit("reseek-agent.service")
        pid = main_pid(agent, "reseek-agent")
        owner = agent.succeed(f"ps -o user= -p {pid}").strip()
        assert owner == "reseek-agent", f"the agent runs as {owner}"
        state = until(
            controller,
            "the controller serving the agent's files",
            lambda s: serving(s, len(agent_files)),
        )
        listed = state["relay"]["agents"][0]
        address = agent.succeed("ip -4 -o addr show eth1 | awk '{print $4}' | cut -d/ -f1").strip()
        assert listed["ipAddress"] == address, (
            f"the controller lists the agent from {listed['ipAddress']}, not {address}"
        )
        agent.succeed("journalctl -u reseek-agent | grep -F 'Login succeeded.'")
        # the certificate is checked: an agent whose store lacks the test CA is refused
        agent.succeed("systemctl start untrusting.service")
        agent.wait_until_succeeds(
            "journalctl -u untrusting | grep -F 'invalid peer certificate: UnknownIssuer'",
            timeout=60,
        )
        agent.succeed("systemctl stop untrusting.service")

    with subtest("the peer browses the controller's user and sees the agent's files"):
        def shared(listing: Any) -> set[str]:
            return {d["name"] + "\\" + f["filename"] for d in listing["directories"] for f in d["files"]}

        expected = {remote(path) for path in agent_files}
        until(
            peer,
            "the peer seeing the agent's files",
            lambda listing: shared(listing) == expected,
            f"users/{user(users['controller'])}/browse",
        )

    with subtest("the peer downloads a small file and a large one with the bytes intact"):
        wanted = ["Album/01 small.flac", "Album/Disc 2/02 large.flac", "Ünïcödé Ärtist/03 späces & ñ.flac"]
        ids = download(peer, users["controller"], [(remote(p), agent_files[p]) for p in wanted])
        for path, download_id in zip(wanted, ids):
            done = finished(peer, users["controller"], download_id)
            assert done["bytesTransferred"] == agent_files[path], f"{path}: {done}"
            got = sha256(peer, done["localPath"])
            sent = sha256(agent, f"/srv/music/{path}")
            assert got == sent, f"{path} arrived as {got}, the agent has {sent}"

    with subtest("a file in flight when the agent's network is cut arrives once it is back"):
        cut = "Album/04 cut.flac"
        path = f"transfers/downloads/{user(users['controller'])}"
        [cut_id] = download(peer, users["controller"], [(remote(cut), agent_files[cut])])
        until(
            peer,
            "the download moving bytes",
            lambda t: t["state"] == "InProgress" and t["bytesTransferred"] > 0,
            f"{path}/{cut_id}",
        )
        agent.block()
        until(
            controller,
            "the controller dropping the silent agent and its shares",
            lambda s: s["relay"]["agents"] == [] and s["shares"]["files"] == 0,
        )
        def cut_uploads(groups: list[Any]) -> list[Any]:
            return [u for u in flattened(groups) if u["filename"] == remote(cut)]

        [upload] = cut_uploads(
            until(
                controller,
                "the cut upload ending",
                lambda groups: all(u["state"].startswith("Completed") for u in cut_uploads(groups)),
                "transfers/uploads",
            )
        )
        assert upload["state"] == "Completed, Errored", f"the cut upload: {upload}"
        assert upload["exception"] == "the agent disconnected while it sent the file", (
            f"the cut upload: {upload}"
        )
        # the controller still holds the agent configured, so the peer is told to ask again, not
        # that the file is not shared
        until(
            peer,
            "the download asking again while the agent is away",
            lambda t: t["attempts"] >= 3 and not t["state"].startswith("Completed"),
            f"{path}/{cut_id}",
        )
        agent.unblock()
        done = finished(peer, users["controller"], cut_id, seconds=300)
        got = sha256(peer, done["localPath"])
        sent = sha256(agent, f"/srv/music/{cut}")
        assert got == sent, f"{cut} arrived as {got}, the agent has {sent}"

    with subtest("the agent comes back after the controller restarts, and a file in flight arrives"):
        restart = "Album/05 restart.flac"
        [restart_id] = download(peer, users["controller"], [(remote(restart), agent_files[restart])])
        until(
            peer,
            "the download moving bytes",
            lambda t: t["state"] == "InProgress" and t["bytesTransferred"] > 0,
            f"transfers/downloads/{user(users['controller'])}/{restart_id}",
        )
        before = main_pid(agent, "reseek-agent")
        logins = "journalctl -u reseek-agent | grep -cF 'Login succeeded.'"
        count = int(agent.succeed(logins))
        controller.succeed("systemctl restart reseek.service")
        agent.wait_until_succeeds(f"test $({logins}) -gt {count}", timeout=120)
        until(
            controller,
            "the agent back after the restart",
            lambda s: serving(s, len(agent_files)),
        )
        after = main_pid(agent, "reseek-agent")
        assert after == before, f"the agent was restarted: {before} became {after}"
        # the peer asks again while the controller and then the agent are away, the agent's file
        # known from the index kept across the restart, and resumes once the agent is back
        done = finished(peer, users["controller"], restart_id, seconds=300)
        assert done["attempts"] > 1, f"the download was not cut by the restart: {done}"
        got = sha256(peer, done["localPath"])
        sent = sha256(agent, f"/srv/music/{restart}")
        assert got == sent, f"{restart} arrived as {got}, the agent has {sent}"

    with subtest("a file added to the agent's share reaches the controller"):
        agent.succeed("mkdir -p /srv/music/Added && head -c 1000 /dev/urandom > /srv/music/Added/new.flac")
        until(
            controller,
            "the added file in the controller's shares",
            lambda s: serving(s, len(agent_files) + 1),
        )

    with subtest("a download the controller makes is copied to the agent"):
        listing = api(controller, f"users/{user(users['peer'])}/browse")
        [gift] = [
            (d["name"] + "\\" + f["filename"], f["size"])
            for d in listing["directories"]
            for f in d["files"]
        ]
        [gift_id] = download(controller, users["peer"], [gift])
        done = finished(controller, users["peer"], gift_id)
        relative = done["localPath"].removeprefix("/var/lib/reseek/data/downloads/")
        assert relative != done["localPath"], f"downloaded outside the downloads: {done}"
        copy = f"/var/lib/reseek-agent/data/downloads/{relative}"
        agent.wait_until_succeeds(f"test -f {shlex.quote(copy)}", timeout=60)
        assert sha256(agent, copy) == sha256(controller, done["localPath"]), "the copy differs"
        owner = agent.succeed(f"stat -c %U {shlex.quote(copy)}").strip()
        assert owner == "reseek-agent", f"the copy belongs to {owner}"

    with subtest("an agent with the wrong secret is refused and not listed"):
        connected = api(controller, "relay/agents")
        agent.succeed("systemctl start wrong-secret.service")
        refused = 'reason="login refused: BadCredential" the relay controller refused this agent'
        agent.wait_until_succeeds(
            f"journalctl -u wrong-secret | grep -F {shlex.quote(refused)}", timeout=60
        )
        controller.succeed("journalctl -u reseek | grep -F 'credential mismatch'")
        assert api(controller, "relay/agents") == connected, "the refused agent changed the agents"
        agent.succeed("systemctl stop wrong-secret.service")

    with subtest("stopping the agent takes its shares off the controller"):
        agent.succeed("systemctl stop reseek-agent.service")
        state = until(
            controller,
            "the agent's shares gone",
            lambda s: s["relay"]["agents"] == [] and s["shares"]["files"] == 0,
        )
        assert state["shares"]["hosts"] == ["local"], f"hosts: {state['shares']['hosts']}"

    with subtest("no journal holds a secret"):
        for node in [controller, agent, peer, server]:
            journal = node.succeed("journalctl -o verbose --no-pager")
            for secret in secrets:
                assert secret not in journal, f"{node.name}'s journal holds {secret}"
  '';
}
