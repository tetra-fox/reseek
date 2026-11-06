import type { Component } from "svelte";

import { app } from "#lib/app.svelte.js";
import PortMapping from "#lib/components/settings/PortMapping.svelte";

/**
 * The control an option is edited with. Most follow from the kind the daemon reports for the key;
 * a description names one only where the kind alone can't say (a secret, a list of patterns or
 * networks); an option that is one of a fixed set of names is a select of the names the daemon
 * gives, and a notification mode is a row of its four choices
 */
export type Control =
  | "text"
  | "number"
  | "bool"
  | "select"
  | "notify-mode"
  | "list"
  | "duration"
  | "size"
  | "auto-count"
  | "secret"
  | "regex-list"
  | "cidr-list"
  | "map"
  | "table-list";

/**
 * One option on the settings page. Its help text, apply class, kind and source come from the
 * daemon's view of the configuration, so a description only says how to show and check it
 */
export type Field = {
  key: string;
  label: string;
  control?: Control;
  /** bounds a number must keep */
  min?: number;
  max?: number;
  /** shown in the empty input, such as what not setting the option means */
  placeholder?: string;
  /** editable only while `key` holds one of `values` */
  enabledWhen?: { key: string; values: unknown[] };
  /** for an `auto-count` control, the count `auto` stands for on the daemon's machine */
  auto?: () => number | undefined;
  /** a list entry that follows from another option: shown dimmed under the list, never saved */
  derived?: { from: string; entry: (values: unknown) => string | null; note: string };
  /** the subheading it goes under on a big form */
  heading?: string;
  /** for a table, the label of each field of a row, by the field's key */
  columns?: Record<string, string>;
  /** for a map, the label of the column that holds each row's name; Name by default */
  rowName?: string;
};

/** The headings the side list of the settings page gathers its sections under */
export type SectionGroup = "Network" | "Storage" | "Server";

/** A form on the settings page; `extra` shows above its fields, such as a state the options set */
export type Section = {
  id: string;
  title: string;
  group: SectionGroup;
  fields: Field[];
  extra?: Component;
};

const PORT = { min: 1, max: 65535 };
const AT_LEAST_1 = { min: 1 };
const NO_LIMIT = "no limit";

/** The value at a dotted key of the nested values */
export const valueAt = (values: unknown, key: string): unknown =>
  key
    .split(".")
    .reduce<unknown>((at, part) => (at as Record<string, unknown> | null)?.[part], values);

/** `fields` under the subheading `heading` */
const under = (heading: string, fields: Field[]): Field[] =>
  fields.map((field) => ({ ...field, heading }));

const LIMIT_WINDOWS = ["queued", "daily", "weekly"];
const LIMIT_MEASURES = ["files", "megabytes", "failures"];

/** The nine per-user limits under `prefix`: files, megabytes and failures, queued, daily and weekly */
const limits = (prefix: string, who: string): Field[] =>
  LIMIT_WINDOWS.flatMap((window) =>
    LIMIT_MEASURES.map((what) => ({
      key: `${prefix}.limits.${window}.${what}`,
      label: `${who}: ${window} ${what}`,
      ...AT_LEAST_1,
      placeholder: NO_LIMIT
    }))
  );

const capitalized = (word: string) => `${word[0].toUpperCase()}${word.slice(1)}`;

/** The row labels of a user-defined group: its members and what its upload settings are called */
const GROUP_COLUMNS: Record<string, string> = {
  "members": "Members",
  "upload.priority": "Priority",
  "upload.strategy": "Queue order",
  "upload.slots": "Upload slots",
  "upload.speed_limit": "Speed limit (KiB/s)",
  ...Object.fromEntries(
    LIMIT_WINDOWS.flatMap((window) =>
      LIMIT_MEASURES.map((what) => [
        `upload.limits.${window}.${what}`,
        `${capitalized(window)} ${what}`
      ])
    )
  )
};

const OIDC_COLUMNS: Record<string, string> = {
  "issuer": "Issuer URL",
  "client_id": "Client ID",
  "client_secret": "Client secret",
  "display_name": "Button label",
  "scopes": "Scopes",
  "role_mapping.claim": "Groups claim",
  "role_mapping.administrator": "Administrator groups",
  "role_mapping.readwrite": "Read and write groups",
  "role_mapping.readonly": "Read-only groups",
  "role_mapping.default": "Default role",
  "allowed_subjects": "Allowed subjects",
  "allowed_emails": "Allowed email addresses",
  "username_claim": "Username claim",
  "auto_login": "Log in automatically",
  "session_ttl": "Session lifetime",
  "logout.rp_initiated": "Log out at the provider",
  "logout.back_channel": "Accept back-channel logout",
  "ca_certificate": "Certificate authority file",
  "require_https": "Require HTTPS"
};

/** A group's upload settings under `prefix` */
const groupUpload = (prefix: string, group: string): Field[] => [
  { key: `${prefix}.upload.priority`, label: `${group} priority`, ...AT_LEAST_1 },
  {
    key: `${prefix}.upload.strategy`,
    label: `${group} queue order`
  },
  { key: `${prefix}.upload.slots`, label: `${group} upload slots`, ...AT_LEAST_1 },
  { key: `${prefix}.upload.speed_limit`, label: `${group} speed limit (KiB/s)`, ...AT_LEAST_1 },
  ...limits(`${prefix}.upload`, group)
];

/** The four outcome ages of one transfer direction's retention, by the option key's outcome */
const transferRetention = (direction: "upload" | "download"): Field[] =>
  ["succeeded", "errored", "canceled", "failed"].map((outcome) => ({
    key: `retention.transfers.${direction}.${outcome}`,
    label: `${direction === "upload" ? "Uploads" : "Downloads"} ${outcome}`,
    placeholder: "kept"
  }));

/**
 * Options Settings does not show: `relay.mode` takes only `controller` and is kept so that a
 * configuration still naming the daemon's old roles fails with a pointer to `reseek-agent`;
 * whether the daemon is a controller is `relay.enabled`
 */
export const unlisted = new Set(["relay.mode"]);

/** The forms of the settings page, one per part of the configuration, in this order */
export const sections: Section[] = [
  {
    id: "soulseek",
    title: "Soulseek",
    group: "Network",
    extra: PortMapping,
    fields: [
      ...under("Account", [
        { key: "soulseek.username", label: "Username" },
        { key: "soulseek.password", label: "Password", control: "secret" },
        { key: "soulseek.description", label: "Profile text" },
        { key: "soulseek.picture", label: "Profile picture" }
      ]),
      ...under("Server", [
        { key: "soulseek.address", label: "Server" },
        { key: "soulseek.port", label: "Server port", min: 1024, max: 65535 },
        {
          key: "soulseek.connection.mtu",
          label: "Server connection MTU",
          min: 576,
          max: 65535,
          placeholder: "system default"
        },
        { key: "soulseek.server_keepalive.enabled", label: "Server keep-alive" },
        ...(["idle", "interval", "probes"] as const).map((part): Field => ({
          key: `soulseek.server_keepalive.${part}`,
          label: `Keep-alive ${part}`,
          ...(part === "probes" ? AT_LEAST_1 : {}),
          enabledWhen: { key: "soulseek.server_keepalive.enabled", values: [true] }
        }))
      ]),
      ...under("Listening", [
        { key: "soulseek.listen_ip_address", label: "Listen address" },
        { key: "soulseek.listen_port", label: "Listen port", min: 1024, max: 65535 },
        { key: "soulseek.port_mapping", label: "Port mapping" }
      ]),
      ...under("Distributed network", [
        { key: "soulseek.distributed_network.disabled", label: "Disable distributed network" },
        { key: "soulseek.distributed_network.disable_children", label: "Refuse child connections" },
        {
          key: "soulseek.distributed_network.child_limit",
          label: "Maximum child connections",
          ...AT_LEAST_1
        },
        {
          key: "soulseek.limits.distributed_parent_negotiation_timeout",
          label: "Distributed parent timeout"
        }
      ]),
      ...under("Connections", [
        { key: "soulseek.connection.timeout.connect", label: "Connect timeout" },
        { key: "soulseek.connection.timeout.inactivity", label: "Idle timeout" },
        { key: "soulseek.connection.timeout.transfer", label: "Transfer stall timeout" },
        {
          key: "soulseek.connection.timeout.transfer_response",
          label: "Transfer response timeout"
        },
        { key: "soulseek.browse.response_timeout", label: "Browse response timeout" },
        { key: "soulseek.connection.buffer.read", label: "Read buffer (bytes)", min: 1024 },
        { key: "soulseek.connection.buffer.write", label: "Write buffer (bytes)", min: 1024 },
        {
          key: "soulseek.connection.buffer.transfer",
          label: "Transfer buffer (bytes)",
          min: 81920
        },
        { key: "soulseek.connection.buffer.write_queue", label: "Write queue", min: 5, max: 5000 }
      ]),
      ...under("Proxy", [
        { key: "soulseek.connection.proxy.enabled", label: "SOCKS5 proxy" },
        ...(["address", "port", "username", "password"] as const).map((part): Field => ({
          key: `soulseek.connection.proxy.${part}`,
          label: `Proxy ${part}`,
          ...(part === "port" ? PORT : {}),
          ...(part === "password" ? { control: "secret" as const } : {}),
          enabledWhen: { key: "soulseek.connection.proxy.enabled", values: [true] }
        }))
      ]),
      ...under("Limits", [
        { key: "soulseek.search.max_concurrent", label: "Concurrent searches", ...AT_LEAST_1 },
        { key: "soulseek.limits.max_message_size", label: "Maximum peer message size" },
        {
          key: "soulseek.limits.max_concurrent_peer_connects",
          label: "Concurrent peer connection attempts",
          ...AT_LEAST_1
        },
        {
          key: "soulseek.limits.max_cached_peer_connections",
          label: "Cached peer connections",
          ...AT_LEAST_1
        },
        {
          key: "soulseek.limits.search_response_cache_size",
          label: "Cached search responses",
          ...AT_LEAST_1
        },
        {
          key: "soulseek.limits.user_endpoint_cache_size",
          label: "Cached user addresses",
          ...AT_LEAST_1
        }
      ]),
      ...under("Diagnostics", [
        { key: "soulseek.diagnostic_level", label: "Protocol diagnostics" },
        { key: "soulseek.distributed_network.logging", label: "Distributed network diagnostics" }
      ])
    ]
  },
  {
    id: "shares",
    title: "Shares",
    group: "Network",
    fields: [
      {
        key: "shares.directories",
        label: "Shared folders",
        derived: {
          from: "shares.share_downloads",
          entry: (values) =>
            valueAt(values, "shares.share_downloads") === true
              ? String(valueAt(values, "directories.downloads") ?? "the downloads folder")
              : null,
          note: "From Share the downloads folder (shares.share_downloads)"
        }
      },
      { key: "shares.share_downloads", label: "Share the downloads folder" },
      { key: "shares.buddies_only", label: "Shares only buddies see, by alias" },
      { key: "shares.trusted_only", label: "Shares only trusted buddies see, by alias" },
      { key: "shares.filters", label: "Excluded files", control: "regex-list" },
      { key: "shares.follow_symlinks", label: "Follow symbolic links" },
      { key: "shares.cache.retention", label: "Rescan", placeholder: "off, 12h or 7d at 04:00" },
      {
        key: "shares.cache.storage_mode",
        label: "Share index storage"
      },
      {
        key: "shares.cache.workers",
        label: "Scan workers",
        min: 1,
        max: 128,
        placeholder: "one per processor"
      }
    ]
  },
  {
    id: "transfers",
    title: "Transfers",
    group: "Network",
    fields: [
      ...under("Downloads", [
        {
          key: "transfers.download.slots",
          label: "Download slots",
          ...AT_LEAST_1,
          placeholder: NO_LIMIT
        },
        {
          key: "transfers.download.speed_limit",
          label: "Download speed limit (KiB/s)",
          ...AT_LEAST_1,
          placeholder: NO_LIMIT
        },
        {
          key: "transfers.download.alt_speed_limit",
          label: "Alternative download speed limit (KiB/s)",
          ...AT_LEAST_1,
          placeholder: NO_LIMIT
        },
        { key: "transfers.download.request_style", label: "Download request style" },
        { key: "transfers.download.retry.attempts", label: "Download attempts", ...AT_LEAST_1 },
        { key: "transfers.download.retry.delay", label: "First retry delay" },
        { key: "transfers.download.retry.max_delay", label: "Maximum retry delay" },
        { key: "transfers.download.retry.partial", label: "Partial file on retry" },
        {
          key: "transfers.download.destination.subdirectory",
          label: "Download subdirectory",
          placeholder: "${SOURCE_DIRECTORY}"
        },
        { key: "transfers.download.destination.exists", label: "If the file exists" },
        {
          key: "transfers.download.filters",
          label: "Files skipped in folder downloads",
          control: "regex-list"
        },
        {
          key: "transfers.download.destination.permissions.mode",
          label: "File mode",
          placeholder: "system default"
        },
        { key: "transfers.ui.queue_refresh", label: "Queue position refresh interval" }
      ]),
      ...under("Uploads", [
        { key: "transfers.upload.slots", label: "Upload slots", ...AT_LEAST_1 },
        {
          key: "transfers.upload.speed_limit",
          label: "Upload speed limit (KiB/s)",
          ...AT_LEAST_1,
          placeholder: NO_LIMIT
        },
        {
          key: "transfers.upload.alt_speed_limit",
          label: "Alternative upload speed limit (KiB/s)",
          ...AT_LEAST_1,
          placeholder: NO_LIMIT
        }
      ]),
      ...under("Limits", limits("transfers.upload", "Per user"))
    ]
  },
  {
    id: "groups",
    title: "Groups",
    group: "Network",
    fields: [
      ...under("Default group", groupUpload("transfers.groups.default", "Default")),
      ...under("Leechers", [
        ...groupUpload("transfers.groups.leechers", "Leechers"),
        {
          key: "transfers.groups.leechers.thresholds.files",
          label: "Leechers: minimum shared files",
          ...AT_LEAST_1
        },
        {
          key: "transfers.groups.leechers.thresholds.directories",
          label: "Leechers: minimum shared folders",
          ...AT_LEAST_1
        }
      ]),
      ...under("Blocked", [
        { key: "transfers.groups.blacklisted.members", label: "Blocked users" },
        {
          key: "transfers.groups.blacklisted.patterns",
          label: "Blocked name patterns",
          control: "regex-list"
        },
        {
          key: "transfers.groups.blacklisted.cidrs",
          label: "Blocked networks",
          control: "cidr-list"
        },
        { key: "blocking.default_scopes", label: "Default block scopes" },
        { key: "blocking.refusal_message", label: "Message to blocked users" },
        { key: "blacklist.enabled", label: "Address blocklist" },
        {
          key: "blacklist.file",
          label: "Blocklist file",
          enabledWhen: { key: "blacklist.enabled", values: [true] }
        }
      ]),
      ...under("User-defined groups", [
        {
          key: "transfers.groups.user_defined",
          label: "User-defined groups",
          columns: GROUP_COLUMNS
        }
      ])
    ]
  },
  {
    id: "chat",
    title: "Chat",
    group: "Network",
    fields: [
      { key: "rooms", label: "Rooms to join" },
      {
        key: "chat.accept_private_room_invitations",
        label: "Accept private room invitations"
      },
      { key: "chat.highlight_words", label: "Highlighted words" },
      { key: "chat.max_message_length", label: "Maximum message length" },
      { key: "chat.auto_away", label: "Away after no one has the page open for" },
      { key: "chat.auto_reply", label: "Reply while away" }
    ]
  },
  {
    id: "notifications",
    title: "Notifications",
    group: "Network",
    fields: [
      { key: "chat.notifications.only_when_hidden", label: "Only while out of view" },
      ...under("Chat", [
        { key: "chat.notifications.private_messages", label: "Private messages" },
        { key: "chat.notifications.mentions", label: "Mentions" },
        { key: "chat.notifications.room_messages", label: "Every message in notifying rooms" }
      ]),
      ...under("Transfers", [
        { key: "notifications.transfers.job_finished", label: "Downloads finished" },
        { key: "notifications.transfers.upload_queued", label: "An upload was queued" }
      ]),
      ...under("Buddies", [
        {
          key: "notifications.buddies.presence",
          label: "A buddy came online, went away or offline"
        }
      ]),
      ...under("Jobs", [
        { key: "notifications.jobs.scans", label: "Share scans finished" },
        { key: "notifications.jobs.analysis", label: "Analyses finished" },
        { key: "notifications.jobs.maintenance", label: "Cleanups and vacuums finished" }
      ]),
      ...under("Search", [
        { key: "notifications.search.wishlist", label: "The wishlist found new files" }
      ])
    ]
  },
  {
    id: "search-filters",
    title: "Search filters",
    group: "Network",
    fields: [
      { key: "filters.search.request", label: "Ignored incoming searches", control: "regex-list" },
      {
        key: "search.filter_presets",
        label: "Filter presets",
        columns: { name: "Name", filter: "Filter" }
      }
    ]
  },
  {
    id: "throttling",
    title: "Throttling",
    group: "Network",
    fields: [
      {
        key: "throttling.search.incoming.concurrency",
        label: "Search workers",
        min: 1,
        max: 100
      },
      {
        key: "throttling.search.incoming.circuit_breaker",
        label: "Search queue size",
        min: 100,
        max: 10000
      },
      {
        key: "throttling.search.incoming.response_file_limit",
        label: "Files per response",
        min: 100,
        max: 5000
      }
    ]
  },
  {
    id: "directories",
    title: "Folders",
    group: "Storage",
    fields: [
      {
        key: "directories.downloads",
        label: "Downloads",
        placeholder: "downloads in the data folder"
      },
      {
        key: "directories.incomplete",
        label: "Downloads in progress",
        placeholder: "incomplete in the data folder"
      },
      { key: "paths.data", label: "Data folder" },
      { key: "paths.cache", label: "Cache folder" },
      { key: "paths.logs", label: "Log folder" },
      { key: "paths.config", label: "Configuration folder" }
    ]
  },
  {
    id: "analysis",
    title: "Analysis",
    group: "Storage",
    fields: [
      { key: "analysis.on_download", label: "Analyze finished downloads" },
      { key: "analysis.extensions", label: "Extensions" },
      {
        key: "analysis.workers",
        label: "Concurrent analyses",
        control: "auto-count",
        min: 1,
        auto: () => app.state?.process.autoAnalysisWorkers
      },
      { key: "analysis.timeout", label: "Time limit per file" },
      {
        key: "analysis.max_full_minutes",
        label: "Longest file analyzed in full (minutes)",
        min: 1
      },
      { key: "analysis.sampled_windows", label: "Windows in a sampled file", min: 20, max: 2000 },
      { key: "analysis.cache_max_bytes", label: "Spectrogram storage" },
      { key: "analysis.upload_cache_max_bytes", label: "Uploaded file storage" },
      { key: "analysis.upload_max_bytes", label: "Largest uploaded file" },
      { key: "analysis.spectrogram.scale", label: "Default frequency scale" }
    ]
  },
  {
    id: "retention",
    title: "Retention",
    group: "Storage",
    fields: [
      ...under("Searches and transfers", [
        { key: "retention.search", label: "Searches", placeholder: "kept" },
        ...transferRetention("upload"),
        ...transferRetention("download"),
        {
          key: "retention.transfers.purge_removed_after",
          label: "Removed transfers",
          placeholder: "kept"
        }
      ]),
      ...under("Files", [
        { key: "retention.files.complete", label: "Downloaded files", placeholder: "kept" },
        { key: "retention.files.incomplete", label: "Incomplete files", placeholder: "kept" }
      ]),
      ...under("Events and logs", [
        { key: "retention.events", label: "Events" },
        { key: "retention.events_max_rows", label: "Maximum events", min: 0 },
        { key: "retention.logs", label: "Log files" }
      ]),
      ...under("Messages", [
        { key: "retention.messages.private", label: "Private messages" },
        {
          key: "retention.messages.max_per_conversation",
          label: "Maximum per conversation",
          min: 0
        },
        { key: "retention.messages.rooms", label: "Room messages" },
        { key: "retention.messages.mentions", label: "Mentions" },
        { key: "retention.messages.max_per_room", label: "Maximum per room", min: 0 }
      ]),
      ...under("Jobs", [
        { key: "retention.jobs.age", label: "Ended jobs" },
        { key: "retention.jobs.count", label: "Maximum ended jobs", min: 0 }
      ]),
      ...under("Users", [
        { key: "retention.users", label: "Users not seen" },
        { key: "users.cache_pictures", label: "Keep profile pictures" },
        { key: "users.picture_max_bytes", label: "Largest profile picture kept" }
      ])
    ]
  },
  {
    id: "schedules",
    title: "Schedules",
    group: "Storage",
    fields: [
      { key: "schedules.cleanup", label: "Cleanup", placeholder: "off, 5m or 1d at 04:00" },
      { key: "schedules.vacuum", label: "Database vacuum", placeholder: "off, 7d at 04:00" },
      { key: "schedules.release_check", label: "Release check", placeholder: "off, 1d" }
    ]
  },
  {
    id: "web",
    title: "Web",
    group: "Server",
    fields: [
      ...under("HTTP", [
        { key: "web.port", label: "HTTP port", ...PORT },
        { key: "web.ip_address", label: "HTTP addresses", placeholder: "all addresses" },
        { key: "web.socket", label: "Unix socket" },
        { key: "web.url_base", label: "URL base" },
        { key: "web.external_url", label: "Public URL", placeholder: "derived from each request" },
        { key: "web.hostnames", label: "Host names" },
        { key: "web.trusted_proxies", label: "Trusted proxies", control: "cidr-list" },
        { key: "web.cors.allowed_origins", label: "Allowed origins" },
        { key: "web.logging", label: "Request logging" }
      ]),
      ...under("HTTPS", [
        { key: "web.https.disabled", label: "Disable HTTPS" },
        { key: "web.https.port", label: "HTTPS port", ...PORT },
        { key: "web.https.ip_address", label: "HTTPS addresses", placeholder: "all addresses" },
        { key: "web.https.force", label: "Redirect to HTTPS" },
        { key: "web.https.certificate.cert", label: "Certificate (PEM)" },
        { key: "web.https.certificate.key", label: "Private key (PEM)" },
        { key: "web.https.certificate.pfx", label: "Certificate (PKCS#12)" },
        { key: "web.https.certificate.password", label: "PKCS#12 password", control: "secret" }
      ]),
      ...under("Web UI", [
        { key: "web.ui", label: "Web UI" },
        { key: "web.content_path", label: "UI directory", placeholder: "built-in UI" },
        {
          key: "web.live.max_streams_per_principal",
          label: "Event streams per user",
          ...AT_LEAST_1
        },
        { key: "feature.swagger", label: "API browser" }
      ]),
      ...under("Serving files", [
        { key: "web.files.serve", label: "Serve downloaded files" },
        { key: "web.files.archive_max_bytes", label: "Maximum folder zip size" },
        {
          key: "web.files.serve_speed_limit",
          label: "File serving speed limit (KiB/s)",
          ...AT_LEAST_1,
          placeholder: NO_LIMIT
        }
      ])
    ]
  },
  {
    id: "authentication",
    title: "Authentication",
    group: "Server",
    fields: [
      ...under("Administrator", [
        { key: "web.authentication.username", label: "Administrator username" },
        { key: "web.authentication.password", label: "Administrator password", control: "secret" },
        { key: "web.authentication.password_hash", label: "Password hash", control: "secret" }
      ]),
      ...under("Password login", [
        { key: "web.authentication.local.enabled", label: "Password login" },
        {
          key: "web.authentication.local.only_from",
          label: "Password login from",
          control: "cidr-list"
        },
        {
          key: "web.authentication.local.users",
          label: "Additional accounts",
          rowName: "Username",
          columns: { password_hash: "Password hash", password: "Password", role: "Role" }
        }
      ]),
      ...under("Sessions", [
        { key: "web.authentication.session_ttl", label: "Session lifetime" },
        { key: "web.authentication.session_idle", label: "Session idle timeout" }
      ]),
      ...under("Trusted access", [
        {
          key: "web.authentication.trusted_networks",
          label: "Trusted networks",
          control: "cidr-list"
        },
        { key: "web.authentication.trusted_role", label: "Trusted network role" },
        { key: "web.authentication.disabled", label: "Disable authentication" }
      ]),
      ...under("OpenID Connect", [
        {
          key: "web.authentication.oidc",
          label: "OpenID Connect providers",
          columns: OIDC_COLUMNS
        }
      ]),
      ...under("Reverse proxy", [
        { key: "web.authentication.proxy.enabled", label: "Proxy authentication" },
        ...(["user_header", "email_header", "groups_header"] as const).map((part): Field => ({
          key: `web.authentication.proxy.${part}`,
          label: `Proxy ${part.replace("_", " ")}`,
          enabledWhen: { key: "web.authentication.proxy.enabled", values: [true] }
        })),
        {
          key: "web.authentication.proxy.session",
          label: "Proxy sessions",
          enabledWhen: { key: "web.authentication.proxy.enabled", values: [true] }
        },
        ...(["administrator", "readwrite", "readonly"] as const).map((role): Field => ({
          key: `web.authentication.proxy.role_mapping.${role}`,
          label: `Proxy groups: ${role}`,
          enabledWhen: { key: "web.authentication.proxy.enabled", values: [true] }
        })),
        {
          key: "web.authentication.proxy.role_mapping.default",
          label: "Proxy default role",
          enabledWhen: { key: "web.authentication.proxy.enabled", values: [true] }
        }
      ]),
      ...under("API keys", [
        { key: "web.authentication.api_key", label: "API key", control: "secret" },
        {
          key: "web.authentication.api_keys",
          label: "API keys",
          columns: { key: "Key", role: "Role", cidr: "Networks" }
        }
      ])
    ]
  },
  {
    id: "integrations",
    title: "Integrations",
    group: "Server",
    fields: [
      {
        key: "integrations.scripts",
        label: "Scripts",
        columns: {
          "on": "Events",
          "run.command": "Shell command",
          "run.executable": "Program",
          "run.args": "Arguments",
          "run.arglist": "Argument list",
          "timeout": "Time limit"
        }
      }
    ]
  },
  {
    id: "logging",
    title: "Logging",
    group: "Server",
    fields: [
      {
        key: "logging.level",
        label: "Log level"
      },
      { key: "logging.targets", label: "Module log levels" },
      { key: "logger.no_disk", label: "Disable log files" },
      { key: "logger.no_color", label: "Disable console color" },
      { key: "logger.loki", label: "Loki URL" }
    ]
  },
  {
    id: "metrics",
    title: "Metrics",
    group: "Server",
    fields: [
      { key: "metrics.enabled", label: "Prometheus metrics" },
      {
        key: "metrics.url",
        label: "Path",
        enabledWhen: { key: "metrics.enabled", values: [true] }
      },
      {
        key: "metrics.authentication.disabled",
        label: "Disable metrics authentication",
        enabledWhen: { key: "metrics.enabled", values: [true] }
      },
      {
        key: "metrics.authentication.username",
        label: "Username",
        enabledWhen: { key: "metrics.enabled", values: [true] }
      },
      {
        key: "metrics.authentication.password",
        label: "Password",
        control: "secret",
        enabledWhen: { key: "metrics.enabled", values: [true] }
      }
    ]
  },
  {
    id: "relay",
    title: "Relay",
    group: "Server",
    fields: [
      { key: "relay.enabled", label: "Relay controller" },
      { key: "instance_name", label: "Instance name" },
      ...under("Agents", [
        {
          key: "relay.agents",
          label: "Agents",
          columns: { instance_name: "Instance name", secret: "Secret", cidr: "Networks" }
        },
        ...(
          [
            ["controller_timeouts.file_info", "File info timeout"],
            ["controller_timeouts.file_ack", "File acknowledgment timeout"],
            ["controller_timeouts.file_stream", "File stream timeout"],
            ["controller_timeouts.sync_deferral", "Share sync deferral"],
            ["limits.share_database_max", "Largest share database"]
          ] as const
        ).map(([part, label]): Field => ({ key: `relay.${part}`, label }))
      ])
    ]
  },
  {
    id: "flags",
    title: "Flags",
    group: "Server",
    fields: [
      { key: "remote_configuration", label: "Remote configuration" },
      { key: "remote_file_management", label: "Remote file management" },
      { key: "debug", label: "Debug mode" },
      { key: "headless", label: "Headless" },
      { key: "shutdown_timeout", label: "Shutdown timeout" },
      ...(
        [
          ["no_connect", "Do not connect at startup"],
          ["no_share_scan", "Skip share scan at startup"],
          ["force_share_scan", "Full share scan at startup"],
          ["no_version_check", "Disable version check"],
          ["no_config_watch", "Disable configuration watch"],
          ["config_watch_poll", "Poll configuration files"],
          ["case_sensitive_regex", "Case-sensitive patterns"],
          ["volatile", "In-memory databases"],
          ["log_sql", "Log SQL"],
          ["optimistic_relay_file_info", "Trust agent share listings"],
          ["force_migrations", "Force migrations"],
          ["legacy_windows_tcp_keepalive", "Legacy Windows keep-alive"],
          ["no_logo", "Hide startup banner"],
          ["no_start", "Check configuration and exit"]
        ] as const
      ).map(([flag, label]): Field => ({ key: `flags.${flag}`, label }))
    ]
  }
];
