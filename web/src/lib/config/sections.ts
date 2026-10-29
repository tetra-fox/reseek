import type { Component } from "svelte";

import PortMapping from "#lib/components/settings/PortMapping.svelte";

/**
 * The control an option is edited with. Most follow from the kind the daemon reports for the key;
 * a description names one only where the kind alone can't say (a secret, a list of patterns or
 * networks); an option that is one of a fixed set of names is a select of the names the daemon
 * gives
 */
export type Control =
  | "text"
  | "number"
  | "bool"
  | "select"
  | "list"
  | "duration"
  | "size"
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
  /** a list entry that follows from another option: shown dimmed under the list, never saved */
  derived?: { from: string; entry: (values: unknown) => string | null; note: string };
};

/** A form on the settings page; `extra` shows above its fields, such as a state the options set */
export type Section = { id: string; title: string; fields: Field[]; extra?: Component };

const PORT = { min: 1, max: 65535 };
const AT_LEAST_1 = { min: 1 };
const NO_LIMIT = "no limit";

/** The value at a dotted key of the nested values */
export const valueAt = (values: unknown, key: string): unknown =>
  key
    .split(".")
    .reduce<unknown>((at, part) => (at as Record<string, unknown> | null)?.[part], values);

/** The nine per-user limits under `prefix`: files, megabytes and failures, queued, daily and weekly */
const limits = (prefix: string, who: string): Field[] =>
  ["queued", "daily", "weekly"].flatMap((window) =>
    ["files", "megabytes", "failures"].map((what) => ({
      key: `${prefix}.limits.${window}.${what}`,
      label: `${who}: ${window} ${what}`,
      ...AT_LEAST_1,
      placeholder: NO_LIMIT
    }))
  );

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

/** The forms of the settings page, one per part of the configuration, in this order */
export const sections: Section[] = [
  {
    id: "soulseek",
    title: "Soulseek",
    extra: PortMapping,
    fields: [
      { key: "soulseek.username", label: "Username" },
      { key: "soulseek.password", label: "Password", control: "secret" },
      { key: "soulseek.address", label: "Server" },
      { key: "soulseek.port", label: "Server port", min: 1024, max: 65535 },
      { key: "soulseek.description", label: "Profile text" },
      { key: "soulseek.picture", label: "Profile picture" },
      { key: "soulseek.listen_ip_address", label: "Listen address" },
      { key: "soulseek.listen_port", label: "Listen port", min: 1024, max: 65535 },
      { key: "soulseek.port_mapping", label: "Port mapping" },
      { key: "rooms", label: "Rooms to join" },
      {
        key: "soulseek.diagnostic_level",
        label: "Protocol diagnostics"
      },
      { key: "soulseek.distributed_network.disabled", label: "Disable distributed network" },
      { key: "soulseek.distributed_network.disable_children", label: "Refuse child connections" },
      {
        key: "soulseek.distributed_network.child_limit",
        label: "Maximum child connections",
        ...AT_LEAST_1
      },
      { key: "soulseek.distributed_network.logging", label: "Distributed network diagnostics" },
      { key: "soulseek.connection.timeout.connect", label: "Connect timeout" },
      { key: "soulseek.connection.timeout.inactivity", label: "Idle timeout" },
      { key: "soulseek.connection.timeout.transfer", label: "Transfer stall timeout" },
      { key: "soulseek.connection.timeout.transfer_response", label: "Transfer response timeout" },
      { key: "soulseek.connection.buffer.read", label: "Read buffer (bytes)", min: 1024 },
      { key: "soulseek.connection.buffer.write", label: "Write buffer (bytes)", min: 1024 },
      { key: "soulseek.connection.buffer.transfer", label: "Transfer buffer (bytes)", min: 81920 },
      { key: "soulseek.connection.buffer.write_queue", label: "Write queue", min: 5, max: 5000 },
      {
        key: "soulseek.connection.mtu",
        label: "Server connection MTU",
        min: 576,
        max: 65535,
        placeholder: "system default"
      },
      { key: "soulseek.connection.proxy.enabled", label: "SOCKS5 proxy" },
      ...(["address", "port", "username", "password"] as const).map((part): Field => ({
        key: `soulseek.connection.proxy.${part}`,
        label: `Proxy ${part}`,
        ...(part === "port" ? PORT : {}),
        ...(part === "password" ? { control: "secret" as const } : {}),
        enabledWhen: { key: "soulseek.connection.proxy.enabled", values: [true] }
      })),
      { key: "soulseek.server_keepalive.enabled", label: "Server keep-alive" },
      ...(["idle", "interval", "probes"] as const).map((part): Field => ({
        key: `soulseek.server_keepalive.${part}`,
        label: `Keep-alive ${part}`,
        ...(part === "probes" ? AT_LEAST_1 : {}),
        enabledWhen: { key: "soulseek.server_keepalive.enabled", values: [true] }
      })),
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
        key: "soulseek.limits.max_concurrent_search_responses",
        label: "Concurrent distributed search responses",
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
      },
      {
        key: "soulseek.limits.distributed_parent_negotiation_timeout",
        label: "Distributed parent timeout"
      }
    ]
  },
  {
    id: "shares",
    title: "Shares",
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
      { key: "shares.filters", label: "Excluded files", control: "regex-list" },
      { key: "shares.follow_symlinks", label: "Follow symbolic links" },
      { key: "shares.cache.retention", label: "Rescan interval", placeholder: "never" },
      {
        key: "shares.cache.storage_mode",
        label: "Share index storage"
      },
      { key: "shares.cache.workers", label: "Scan workers", min: 1, max: 128 }
    ]
  },
  {
    id: "transfers",
    title: "Transfers",
    fields: [
      { key: "transfers.upload.slots", label: "Upload slots", ...AT_LEAST_1 },
      {
        key: "transfers.upload.speed_limit",
        label: "Upload speed limit (KiB/s)",
        ...AT_LEAST_1,
        placeholder: NO_LIMIT
      },
      ...limits("transfers.upload", "Per user"),
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
        key: "transfers.download.request_style",
        label: "Download request style"
      },
      { key: "transfers.download.retry.attempts", label: "Download attempts", ...AT_LEAST_1 },
      { key: "transfers.download.retry.delay", label: "First retry delay" },
      { key: "transfers.download.retry.max_delay", label: "Maximum retry delay" },
      {
        key: "transfers.download.retry.partial",
        label: "Partial file on retry"
      },
      {
        key: "transfers.download.destination.subdirectory",
        label: "Download subdirectory",
        placeholder: "${SOURCE_DIRECTORY}"
      },
      {
        key: "transfers.download.destination.exists",
        label: "If the file exists"
      },
      {
        key: "transfers.download.destination.permissions.mode",
        label: "File mode",
        placeholder: "system default"
      },
      { key: "transfers.ui.queue_refresh", label: "Queue position refresh interval" }
    ]
  },
  {
    id: "groups",
    title: "Groups",
    fields: [
      ...groupUpload("transfers.groups.default", "Default"),
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
      },
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
      { key: "transfers.groups.user_defined", label: "User-defined groups" },
      { key: "blacklist.enabled", label: "Address blocklist" },
      {
        key: "blacklist.file",
        label: "Blocklist file",
        enabledWhen: { key: "blacklist.enabled", values: [true] }
      }
    ]
  },
  {
    id: "directories",
    title: "Folders",
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
      }
    ]
  },
  {
    id: "analysis",
    title: "Analysis",
    fields: [
      { key: "analysis.on_download", label: "Analyze finished downloads" },
      { key: "analysis.extensions", label: "Extensions" },
      { key: "analysis.workers", label: "Concurrent analyses", min: 1, max: 4 },
      { key: "analysis.timeout", label: "Time limit per file" },
      {
        key: "analysis.max_full_minutes",
        label: "Longest file analyzed in full (minutes)",
        min: 1
      },
      { key: "analysis.sampled_windows", label: "Windows in a sampled file", min: 20, max: 2000 },
      { key: "analysis.cache_max_bytes", label: "Spectrogram storage" },
      { key: "analysis.spectrogram.scale", label: "Default frequency scale" }
    ]
  },
  {
    id: "web",
    title: "Web",
    fields: [
      { key: "web.port", label: "HTTP port", ...PORT },
      { key: "web.ip_address", label: "HTTP addresses", placeholder: "all addresses" },
      { key: "web.socket", label: "Unix socket" },
      { key: "web.url_base", label: "URL base" },
      { key: "web.external_url", label: "Public URL", placeholder: "derived from each request" },
      { key: "web.hostnames", label: "Host names" },
      { key: "web.trusted_proxies", label: "Trusted proxies", control: "cidr-list" },
      { key: "web.ui", label: "Web UI" },
      { key: "web.content_path", label: "UI directory", placeholder: "built-in UI" },
      { key: "web.cors.allowed_origins", label: "Allowed origins" },
      { key: "web.logging", label: "Request logging" },
      { key: "web.https.disabled", label: "Disable HTTPS" },
      { key: "web.https.port", label: "HTTPS port", ...PORT },
      { key: "web.https.ip_address", label: "HTTPS addresses", placeholder: "all addresses" },
      { key: "web.https.force", label: "Redirect to HTTPS" },
      { key: "web.https.certificate.cert", label: "Certificate (PEM)" },
      { key: "web.https.certificate.key", label: "Private key (PEM)" },
      { key: "web.https.certificate.pfx", label: "Certificate (PKCS#12)" },
      { key: "web.https.certificate.password", label: "PKCS#12 password", control: "secret" },
      { key: "web.live.max_streams_per_principal", label: "Event streams per user", ...AT_LEAST_1 },
      { key: "web.files.serve", label: "Serve downloaded files" },
      { key: "web.files.archive_max_bytes", label: "Maximum folder zip size" },
      {
        key: "web.files.serve_speed_limit",
        label: "File serving speed limit (KiB/s)",
        ...AT_LEAST_1,
        placeholder: NO_LIMIT
      },
      { key: "feature.swagger", label: "API browser" }
    ]
  },
  {
    id: "authentication",
    title: "Authentication",
    fields: [
      { key: "web.authentication.username", label: "Administrator username" },
      { key: "web.authentication.password", label: "Administrator password", control: "secret" },
      { key: "web.authentication.password_hash", label: "Password hash", control: "secret" },
      { key: "web.authentication.local.enabled", label: "Password login" },
      {
        key: "web.authentication.local.only_from",
        label: "Password login from",
        control: "cidr-list"
      },
      { key: "web.authentication.local.users", label: "Additional accounts" },
      { key: "web.authentication.session_ttl", label: "Session lifetime" },
      { key: "web.authentication.session_idle", label: "Session idle timeout" },
      {
        key: "web.authentication.trusted_networks",
        label: "Trusted networks",
        control: "cidr-list"
      },
      {
        key: "web.authentication.trusted_role",
        label: "Trusted network role"
      },
      { key: "web.authentication.disabled", label: "Disable authentication" },
      { key: "web.authentication.oidc", label: "OpenID Connect providers" },
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
      },
      { key: "web.authentication.api_key", label: "API key", control: "secret" },
      { key: "web.authentication.api_keys", label: "API keys" }
    ]
  },
  {
    id: "search-filters",
    title: "Search filters",
    fields: [
      { key: "filters.search.request", label: "Ignored incoming searches", control: "regex-list" },
      { key: "search.filter_presets", label: "Filter presets" }
    ]
  },
  {
    id: "retention",
    title: "Retention",
    fields: [
      { key: "retention.search", label: "Searches", placeholder: "kept" },
      ...transferRetention("upload"),
      ...transferRetention("download"),
      {
        key: "retention.transfers.purge_removed_after",
        label: "Removed transfers",
        placeholder: "kept"
      },
      { key: "retention.files.complete", label: "Downloaded files", placeholder: "kept" },
      { key: "retention.files.incomplete", label: "Incomplete files", placeholder: "kept" },
      { key: "retention.events", label: "Events" },
      { key: "retention.events_max_rows", label: "Maximum events", min: 0 },
      { key: "retention.logs", label: "Log files" },
      { key: "retention.messages.private", label: "Private messages" },
      { key: "retention.messages.max_per_conversation", label: "Maximum per conversation", min: 0 },
      { key: "retention.messages.rooms", label: "Room messages" },
      { key: "retention.messages.mentions", label: "Mentions" },
      { key: "retention.messages.max_per_room", label: "Maximum per room", min: 0 },
      { key: "retention.users", label: "Users not seen" }
    ]
  },
  {
    id: "throttling",
    title: "Throttling",
    fields: [
      {
        key: "throttling.search.incoming.concurrency",
        label: "Concurrent search responses",
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
    id: "integrations",
    title: "Integrations",
    fields: [{ key: "integrations.scripts", label: "Scripts" }]
  },
  {
    id: "logging",
    title: "Logging",
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
    fields: [
      { key: "relay.enabled", label: "Relay mode" },
      {
        key: "relay.mode",
        label: "Relay role"
      },
      { key: "instance_name", label: "Instance name" },
      { key: "relay.agents", label: "Agents" },
      ...(
        [
          ["controller_timeouts.file_info", "File info timeout"],
          ["controller_timeouts.file_ack", "File acknowledgment timeout"],
          ["controller_timeouts.file_stream", "File stream timeout"],
          ["controller_timeouts.sync_deferral", "Share sync deferral"],
          ["limits.share_database_max", "Largest share database"]
        ] as const
      ).map(([part, label]): Field => ({
        key: `relay.${part}`,
        label,
        enabledWhen: { key: "relay.mode", values: ["controller", "debug"] }
      })),
      ...(
        [
          ["address", "Controller URL"],
          ["api_key", "Controller API key"],
          ["secret", "Controller secret"],
          ["ignore_certificate_errors", "Accept any certificate"],
          ["downloads", "Fetch finished downloads"],
          ["downloads_filter", "Download filter"]
        ] as const
      ).map(([part, label]): Field => ({
        key: `relay.controller.${part}`,
        label,
        ...(part === "api_key" || part === "secret" ? { control: "secret" as const } : {}),
        enabledWhen: { key: "relay.mode", values: ["agent", "debug"] }
      }))
    ]
  },
  {
    id: "flags",
    title: "Flags",
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
          ["log_unobserved_exceptions", "Log background failures"],
          ["experimental", "Experimental features"],
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
