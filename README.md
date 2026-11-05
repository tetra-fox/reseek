# reseek

Soulseek client & daemon
<!-- badges: ci, latest release, ghcr, license -->

## Features

- **Search & Transfers** - Standard features. Of course you need these.
- **Chat: rooms, private messages, search** - A more modern take on chat, more akin to Discord or Slack.
- **File management** - Browse, save and delete files right in the browser, instead of downloading to your server and then digging them out over a Samba share or scp.
- **Audio analysis and spectrogram** - A feature-rich file inspector to pick out bad transcodes. e.g. a wav-converted 128k YouTube/SoundCloud rip (which I run into way too often. Shame on those who do this)
- **Users and buddies** - Similar to Nicotine+'s feature set, with buddy shares, blocking and rich user management.
- **Settings UI** - Everything is configurable from within reseek, with an easy way to tell whether it was set by an environment variable, a command line argument, or Nix with a convenient `Copy as Nix` button. First class Nix support! Finally!
- **API** - An API for scripting reseek, with an optional Swagger UI for exploring it. (available at `/swagger`. off by default, enable with `--swagger`)
- **Relay** - Run reseek on a server and your PC can add files to it while connected.
- **OpenID Connect** - Integrate directly with your OAuth provider such as Authentik, with proper permission sets so your friends can't delete your files or get you banned from chat.

## Quick start

Grab the [compose file](#docker-compose) below, fill in your Soulseek login and a password for the web UI, `docker compose up -d`, open <http://localhost:5030>.

The web UI login is `reseek` / `reseek` until you set `RESEEK_PASSWORD`. reseek refuses to start with that default anywhere other machines can reach it, which in a container is always; on a plain binary, keep it on `--http-ip-address 127.0.0.1` with either `--no-https` or `--https-ip-address 127.0.0.1`, or set a password.

## Deployment

reseek runs fine with almost no config, you just need to tell it who you are and what to share. Everything else lives in the Settings page once it's up.

Forward 50300/tcp on your router. It'll technically work without it, but a lot of people won't be able to download from you, and that's not very Soulseek of you.

| Env var                | CLI                 | Purpose                               |
| ---------------------- | ------------------- | ------------------------------------- |
| `RESEEK_SLSK_USERNAME` | `--slsk-username`   | Soulseek account, required to connect |
| `RESEEK_SLSK_PASSWORD` | `--slsk-password`   |                                       |
| `RESEEK_USERNAME`      | `-u`, `--username`  | web UI login                          |
| `RESEEK_PASSWORD`      | `-p`, `--password`  | or `RESEEK_PASSWORD_HASH`             |
| `RESEEK_SHARED_DIR`    | `-s`, `--shared`    | shared folder                         |
| `RESEEK_DOWNLOADS_DIR` | `-o`, `--downloads` |                                       |
| `RESEEK_APP_DIR`       | `--app-dir`         | databases, logs, config               |

Ports: 5030 http, 5031 https, 50300 Soulseek.

There are a lot more options than this. Settings in the UI shows all of them, what they do, and where each one got its value from.

### Docker Compose

```yaml
services:
  reseek:
    image: ghcr.io/tetra-fox/reseek:latest
    restart: unless-stopped
    ports:
      - 5030:5030
      - 50300:50300
    environment:
      RESEEK_SLSK_USERNAME: name
      RESEEK_SLSK_PASSWORD: password
      RESEEK_USERNAME: admin
      RESEEK_PASSWORD: password
      RESEEK_SHARED_DIR: /music
    volumes:
      - ./reseek:/app
      - /path/to/music:/music:ro
```

The container runs as 1000:1000; `PUID` and `PGID` change that. An application directory a container running as root wrote is root's, so give it to the new user once with `chown -R 1000:1000 ./reseek`, or set `PUID` and `PGID` to 0 to keep running as root.

### Nix

```nix
{
  inputs.reseek.url = "github:tetra-fox/reseek";

  # in your configuration, with reseek.nixosModules.default imported
  services.reseek = {
    enable = true;
    openFirewall = true;
    # RESEEK_SLSK_PASSWORD and RESEEK_PASSWORD, kept out of the store
    # A secrets manager such as sops-nix or agenix is recommended.
    environmentFile = "/var/lib/secrets/reseek.env";
    settings = {
      soulseek.username = "name";
      shares.directories = [ "/srv/music" ];
    };
  };
}
```

Alternatively, a Home Manager module is available in `homeManagerModules.default` if you'd rather run it as your user.

### Plain binary

Download the one for your system from [Releases](https://github.com/tetra-fox/reseek/releases), there's Linux amd64, arm64, and a universal macOS binary.

```sh
chmod +x reseek-linux-amd64
RESEEK_PASSWORD=password ./reseek-linux-amd64 --slsk-username name --slsk-password password -s /path/to/music
```

On macOS it's `reseek-macos-universal`. It isn't signed with an Apple developer certificate or notarized, so if you downloaded it in a browser, macOS refuses to run it until you clear the quarantine flag (or click Open Anyway under Privacy & Security in System Settings):

```sh
xattr -d com.apple.quarantine reseek-macos-universal
chmod +x reseek-macos-universal
RESEEK_PASSWORD=password ./reseek-macos-universal --slsk-username name --slsk-password password -s /path/to/music
```

Then open <http://localhost:5030>.

### Relay agent

To share files from another machine through your server, run `reseek-agent` there. It's next to `reseek` on the Releases page and in the Nix package. Turn on relay mode on the server and list the agent under `relay.agents` with a secret, then give the agent the server's address, an API key for it, and that secret:

```sh
RESEEK_CONTROLLER_API_KEY=key RESEEK_CONTROLLER_SECRET=secret ./reseek-agent-linux-amd64 \
  --instance-name pc --controller-address https://server:5031 -s /path/to/music
```

It only dials out, so it needs no open port. On NixOS, `services.reseek-agent` runs it as its own user.

### Desktop

Soon.

## Authentication

Three ways in, whichever fits:

- Local accounts, the default.
- API keys, for scripts.
- OpenID Connect, so your SSO can handle it and you can give your friends a read-only role.

If you're putting it behind a reverse proxy, terminate HTTPS there.

## Verifying a release

Each release carries `SHA256SUMS` and its signature `SHA256SUMS.minisig`. With the `reseek.pub`
from this repository:

```sh
minisign -Vm SHA256SUMS -p reseek.pub
sha256sum --check --ignore-missing SHA256SUMS
```

## Building from source

```sh
nix develop
just build
```

Without Nix you'll want stable Rust, Node 26, pnpm and `just`. With Nix you already have them through direnv.

## Why not slskd or slskr?

On slskd: it works, and I ran it for a long time. But I wanted something closer to Nicotine+ as a daemon and so that meant lots of additions: a users directory with profiles and blocking, chat that survives a restart, and a check on whether a "lossless" download really is one. Each of those needs changes inside the daemon, so a new UI wasn't enough, and upstreaming them would mean dropping a very large pull request on a hobbyist maintainer.

slskr is another Rust implementation, with a much bigger scope than I wanted.

## AI usage disclaimer

A lot of this code was written with the use of coding agents (Claude Opus 5.5, with Claude Fable 5.1 as a reviewer). They work from a written spec of the Soulseek protocol and slskd's behavior, and every milestone is reviewed against it by a second model and then by me for structure and readability. The protocol decoders have round-trip tests, property tests and fuzz targets.

I review the shape of the code, not every line of it. If something isn't clear or the review missed it, please open an issue.

## License

AGPL-3.0-only
