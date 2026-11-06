# The NixOS VM test: a machine runs the module's service against a soulfind of its own, with no
# network beyond the VM. It checks the service as a NixOS user meets it (health, login, version,
# the shares scanned, the configuration read-only and managed by NixOS, a drop-in file read as
# one of its sources, a configuration switch reloaded with SIGHUP, runtime overrides with an
# expiry), then runs the daemon's smoke program: the daemon shares, searches, browses, downloads
# and uploads through its API with the soulseek-rs CLI as the other user. Last, the relay agent's
# service logs in to the daemon as its controller and its share is served beside the daemon's.
{
  self,
  testers,
  reseek,
  reseek-smoke,
  soulfind,
  soulseek-rs,
}:
let
  password = "vm-password";
  # test secrets; real ones never go in the store
  agentKey = "vm-agent-key-0123456789";
  agentSecret = "vm-agent-secret-0123456789";
in
testers.runNixOSTest {
  name = "reseek";

  nodes.machine =
    { lib, pkgs, ... }:
    {
      imports = [ self.nixosModules.default ];
      virtualisation.memorySize = 2048;
      virtualisation.cores = 2;
      environment.systemPackages = [ pkgs.curl ];

      systemd.services.soulfind = {
        wantedBy = [ "multi-user.target" ];
        before = [ "reseek.service" ];
        serviceConfig = {
          ExecStart = "${soulfind}/bin/soulfind -p 2242 -d /var/lib/soulfind/soulfind.db";
          StateDirectory = "soulfind";
          DynamicUser = true;
        };
      };

      # two files to share before the daemon's first scan, and the relay agent's one
      systemd.tmpfiles.rules = [
        "d /srv/share 0755 root root -"
        "f+ /srv/share/one.flac 0644 root root - one"
        "f+ /srv/share/two.flac 0644 root root - two"
        "d /srv/agent 0755 root root -"
        "f+ /srv/agent/three.flac 0644 root root - three"
      ];

      services.reseek = {
        enable = true;
        package = reseek;
        # a test secret; a real one never goes in the store
        environmentFile = pkgs.writeText "reseek.env" ''
          RESEEK_PASSWORD=${password}
          RESEEK_SLSK_PASSWORD=vm-soulseek
        '';
        # a drop-in laid over the settings, as a secret rendered by agenix would be
        extraConfigFiles = [
          (pkgs.writeText "rooms.toml" ''
            rooms = [ "nixos" ]
          '')
        ];
        settings = {
          remote_configuration = true;
          soulseek = {
            username = "reseek";
            address = "127.0.0.1";
            port = 2242;
            port_mapping = false;
          };
          web.https.disabled = true;
          web.authentication.username = "admin";
          shares.directories = [ "/srv/share" ];
          flags.no_version_check = true;
          transfers.upload.slots = 2;
          relay = {
            enabled = true;
            agents.nas = {
              instance_name = "nas";
              secret = agentSecret;
            };
          };
          web.authentication.api_keys.agents = {
            key = agentKey;
            role = "readwrite";
          };
        };
      };

      services.reseek-agent = {
        enable = true;
        package = reseek;
        environmentFile = pkgs.writeText "reseek-agent.env" ''
          RESEEK_CONTROLLER_API_KEY=${agentKey}
          RESEEK_CONTROLLER_SECRET=${agentSecret}
        '';
        settings = {
          instance_name = "nas";
          relay.controller.address = "http://127.0.0.1:5030";
          shares.directories = [ "/srv/agent" ];
        };
      };
      # started by the test once the daemon's own checks are done, so its share counts alone
      systemd.services.reseek-agent.wantedBy = lib.mkForce [ ];

      specialisation.changed.configuration = {
        services.reseek.settings.transfers.upload.slots = lib.mkForce 3;
      };
    };

  testScript = ''
    import json
    import time

    base = "http://127.0.0.1:5030"

    def api(method, path, body=None):
        command = (
            f"curl -s -o /tmp/answer -w '%{{http_code}}' -b /tmp/jar -c /tmp/jar "
            f"-H 'Origin: {base}' -X {method} {base}/api/v0{path}"
        )
        if body is not None:
            command += f" -H 'Content-Type: application/json' --data '{json.dumps(body)}'"
        status = int(machine.succeed(command))
        text = machine.succeed("cat /tmp/answer")
        return status, (json.loads(text) if text.strip() else None)

    def eventually(what, check, limit=60):
        started = time.monotonic()
        while True:
            seen = check()
            if seen is True:
                return
            assert time.monotonic() - started < limit, f"never {what}: {seen}"
            time.sleep(1)

    def files():
        return api("GET", "/application")[1]["shares"]["files"]

    def slots():
        return api("GET", "/options")[1]["transfers"]["upload"]["slots"]

    start_all()
    machine.wait_for_unit("soulfind.service")
    machine.wait_for_unit("reseek.service")
    machine.wait_for_open_port(5030)

    with subtest("the daemon answers and signs in its administrator"):
        machine.succeed(f"curl -sf {base}/health")
        status, _ = api("POST", "/session", {"username": "admin", "password": "${password}"})
        assert status == 204, status

    with subtest("it runs the package's version and scanned the two shared files"):
        _, state = api("GET", "/application")
        assert state["version"]["current"].startswith("${reseek.version}"), state["version"]
        eventually("scanned", lambda: files() == 2 or files())

    with subtest("its configuration is managed by NixOS and read-only"):
        _, view = api("GET", "/options/view")
        assert view["managedBy"] == "Nix", view["managedBy"]
        assert view["files"][0]["writable"] is False, view["files"][0]
        revision = view["files"][0]["revision"]
        status, refused = api("PATCH", "/options/file", {"set": {"debug": True}, "revision": revision})
        assert status == 409, status
        assert refused["detail"] == "managed by nixos", refused

    with subtest("a drop-in file is read as a config.d source"):
        _, view = api("GET", "/options/view")
        source = view["fields"]["rooms"]["source"]
        assert source["layer"] == "file", source
        assert source["file"] == "config.d/01-rooms.toml", source
        assert view["values"]["rooms"] == ["nixos"], view["values"]["rooms"]
        assert any(file["name"] == source["file"] for file in view["files"]), view["files"]

    with subtest("a configuration switch reloads the daemon with SIGHUP"):
        assert slots() == 2
        machine.succeed("/run/current-system/specialisation/changed/bin/switch-to-configuration test")
        eventually("reloaded", lambda: slots() == 3 or slots())
        machine.succeed("journalctl -u reseek.service | grep -q 'configuration reload asked for by SIGHUP'")

    with subtest("a runtime override with an expiry takes effect"):
        status, _ = api("PATCH", "/options", {"set": {"transfers.upload.slots": 4}, "expiresIn": "2h"})
        assert status == 200, status
        assert slots() == 4

    with subtest("the smoke run: share, search, browse, download and upload over soulfind"):
        machine.succeed(
            f"${reseek-smoke}/bin/smoke --url {base} --login admin:${password} --ours reseek "
            "--share /srv/share --server 127.0.0.1:2242 "
            "--peer ${soulseek-rs}/bin/soulseek-rs --peer-dir /tmp/peer >&2"
        )

    with subtest("the relay agent's service logs in and its share is served by the daemon"):
        before = files()
        machine.succeed("systemctl start reseek-agent.service")

        def serving():
            _, agents = api("GET", "/relay/agents")
            return any(a["name"] == "nas" and a["ready"] for a in agents) or agents

        eventually("serving the agent", serving)
        eventually("counting its file", lambda: files() == before + 1 or files())
        machine.succeed("journalctl -u reseek-agent.service | grep -q 'Shares uploaded'")
  '';
}
