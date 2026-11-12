import type { Page } from "@playwright/test";

import type { Schemas } from "../../src/lib/api/client";
import { newId } from "../../src/lib/ids";
import { layout, spectra, spectrogram, waveform } from "./pictures";

/** A time on the evening the fakes take place */
const at = (minute: number, second = 0) =>
  new Date(Date.UTC(2026, 9, 27, 18, minute, second)).toISOString();

/** One event stream frame as the daemon writes it */
export const frame = (id: number, event: string, data: unknown) =>
  `id: ${id}\nevent: ${event}\ndata: ${JSON.stringify(data)}\n\n`;

/** The first frame of a live stream over `topics`, for the built-in administrator */
export const helloFrame = (topics: string[]) =>
  frame(1, "hello", {
    principal: { name: "reseek", role: "Administrator" },
    resumed: false,
    epoch: "e2e",
    serverTime: at(30),
    topics,
    version: {
      current: "0.0.0",
      full: "0.0.0",
      latest: null,
      isUpdateAvailable: false,
      isCanary: false,
      isDevelopment: false
    }
  } satisfies Schemas["Hello"]);

/**
 * Marks the test daemon connected to a server, which it never is, so what is offered only while
 * connected shows: the live stream's application frames carry the daemon's own state, connected
 */
export const fakeConnected = async (page: Page) => {
  await page.route("**/api/v0/live**", async (route) => {
    const url = new URL(route.request().url());
    const topics = (url.searchParams.get("topics") ?? "").split(",");
    if (!topics.includes("application")) return route.continue();
    // read from here, where a name the browser maps, as reseek.test, does not resolve
    url.hostname = "127.0.0.1";
    const read = await route.fetch({
      url: url.href.replace(/live\?.*$/, "application"),
      headers: { ...route.request().headers(), accept: "application/json" }
    });
    const state = await read.json();
    const connected = { ...state, server: { ...state.server, isConnected: true } };
    // the app shows once it has the options too, which the daemon's snapshot sends after the state
    const options = await route.fetch({
      url: url.href.replace(/live\?.*$/, "options"),
      headers: { ...route.request().headers(), accept: "application/json" }
    });
    return route.fulfill({
      headers: { "content-type": "text/event-stream" },
      body:
        helloFrame(topics) +
        frame(2, "application.state", connected) +
        frame(2, "application.options", await options.json())
    });
  });
};

const MEMBERS = ["alice", "bob", "carol", "dave", "erin", "frank", "grace", "heidi"];

/** A message `username` said in `roomName`; the built-in account's own, or a mention of it */
export const roomMessage = (
  localId: number,
  timestamp: string,
  username: string,
  message: string,
  roomName = "indie"
) =>
  ({
    localId,
    timestamp,
    username,
    message,
    roomName,
    self: username === "reseek",
    isMention: message.includes("reseek"),
    hidden: false
  }) satisfies Schemas["RoomMessage"];

/** A joined room with nothing unread, as the list of joined rooms has it */
export const joinedRoom = (name: string, rest: Partial<Schemas["JoinedRoom"]> = {}) =>
  ({
    name,
    unread: 0,
    mentions: 0,
    notify: "mentions",
    userCount: 3,
    lastMessageAt: null,
    lastMessage: null,
    lastReadId: 0,
    ...rest
  }) satisfies Schemas["JoinedRoom"];

/** A conversation with `username` with nothing unread, its messages left to their own call */
export const conversation = (username: string, rest: Partial<Schemas["Conversation"]> = {}) =>
  ({
    username,
    isActive: true,
    isServerNotice: false,
    unAcknowledgedMessageCount: 0,
    hasUnAcknowledgedMessages: false,
    notify: "all",
    draft: "",
    lastMessage: null,
    messages: null,
    ...rest
  }) satisfies Schemas["Conversation"];

/** A private message with `username`, read unless `rest` says otherwise */
export const privateMessage = (
  localId: number,
  timestamp: string,
  username: string,
  direction: Schemas["MessageDirection"],
  message: string,
  rest: Partial<Schemas["PrivateMessage"]> = {}
) =>
  ({
    localId,
    id: localId,
    timestamp,
    username,
    direction,
    message,
    isAcknowledged: true,
    wasReplayed: false,
    autoReply: false,
    ...rest
  }) satisfies Schemas["PrivateMessage"];

/** Answers the chat calls with one room, "indie", one conversation, with "alice", and two notices */
export const fakeChat = async (page: Page) => {
  const said: [string, string][] = [
    ["alice", "anyone have the new boards of canada rip?"],
    ["bob", "yes, slsk://bob/Music/Boards%20of%20Canada/Tomorrow's%20Harvest/ in flac"],
    ["bob", "320 too if you want it smaller"],
    ["carol", "/me waves at everyone"],
    ["reseek", "thanks bob, grabbing it now"],
    ["dave", "reseek: did you see https://example.com/review ?"]
  ];
  const roomMessages = said.map(([username, message], n) =>
    roomMessage(n + 1, at(n), username, message)
  );
  const joined = [
    joinedRoom("indie", {
      userCount: MEMBERS.length,
      lastMessageAt: at(5),
      lastMessage: roomMessages.at(-1),
      lastReadId: 6
    })
  ];
  await page.route("**/api/v0/rooms/joined", (route) => route.fulfill({ json: joined }));
  // a pane opened before the joined rooms are read asks the history for the same page
  await page.route("**/api/v0/rooms/history/indie/messages**", (route) =>
    route.fulfill({ json: { messages: roomMessages, hasMore: false, gaps: [] } })
  );
  await page.route("**/api/v0/rooms/joined/indie**", (route) => {
    const url = new URL(route.request().url());
    if (url.pathname.endsWith("/messages")) {
      return route.fulfill({ json: { messages: roomMessages, hasMore: false, gaps: [] } });
    }
    return route.fulfill({
      json: {
        name: "indie",
        isPrivate: false,
        users: MEMBERS.map((username, n) => ({
          username,
          status: n % 3 === 2 ? "Away" : "Online",
          averageSpeed: 400_000 * (n + 1),
          uploadCount: n,
          fileCount: 1000 * (n + 1),
          directoryCount: 40,
          countryCode: ["NL", "DE", "US", "FR"][n % 4],
          slotsFull: false,
          self: false
        })),
        tickers: [
          { username: "alice", ticker: "listening to Geogaddi", hidden: false },
          { username: "bob", ticker: "sharing the whole BoC catalog", hidden: false }
        ],
        messages: null
      } satisfies Schemas["Room"]
    });
  });
  const alice = conversation("alice");
  const notices = [
    "user portapuppy hasn't enabled private room add. please message them and ask them to do so before trying to add them again.",
    "User portapuppy is now a member of room reseek_private_test"
  ].map((message, n) => privateMessage(n + 1, at(20 + n), "server", "In", message));
  const messages = (
    [
      ["In", "hey, are you still sharing the warp records stuff?"],
      ["Out", "yes, browse away"],
      ["In", "thanks!"]
    ] as const
  ).map(([direction, message], n) =>
    privateMessage(n + 1, at(10 + n), "alice", direction, message)
  );
  await page.route("**/api/v0/conversations**", (route) => {
    const url = new URL(route.request().url());
    if (url.pathname.endsWith("/api/v0/conversations")) return route.fulfill({ json: [alice] });
    if (url.pathname.endsWith("/conversations/server/messages")) {
      return route.fulfill({ json: notices });
    }
    if (url.pathname.endsWith("/conversations/server")) {
      return route.fulfill({
        json: conversation("server", { isServerNotice: true, lastMessage: notices.at(-1) })
      });
    }
    if (url.pathname.endsWith("/messages")) return route.fulfill({ json: messages });
    return route.fulfill({ json: alice });
  });
};

/** The id of the finished search `fakeSearch` answers for */
export const SEARCH_ID = "6f1c9a52-0d1e-4c43-9d55-3a1f0b6e7a10";

/** A shared file as a search response or a browse lists it */
export const file = (filename: string, size = 4_000_000) =>
  ({
    filename,
    size,
    code: 1,
    extension: filename.slice(filename.lastIndexOf(".") + 1),
    isLocked: false,
    attributeCount: 0,
    attributes: [],
    bitRate: 320,
    length: 200
  }) satisfies Schemas["File"];

/** `username`'s response to a search, with a free slot and `files` all `size` long */
export const response = (username: string, speed: number, files: string[], size?: number) =>
  ({
    username,
    country: "NL",
    token: 1,
    hasFreeUploadSlot: true,
    uploadSpeed: speed,
    queueLength: 0,
    fileCount: files.length,
    files: files.map((f) => file(f, size)),
    lockedFileCount: 0,
    lockedFiles: []
  }) satisfies Schemas["SearchResponse"];

/** Answers the search page's calls with one finished search and its responses */
export const fakeSearch = async (page: Page) => {
  const responses = [
    response("alice", 3_000_000, [
      "@@a\\Music\\Album\\01 One.flac",
      "@@a\\Music\\Album\\02 Two.flac",
      "@@a\\Music\\Other\\03 Three.mp3"
    ]),
    // the widest speed and size the formatters write, which their columns must hold
    response("bob", 1023.9 * 1024, ["music/x/1.flac", "music/x/2.flac"], 1023.99 * 1024 ** 3)
  ];
  await page.route(`**/api/v0/searches/${SEARCH_ID}**`, (route) => {
    const url = new URL(route.request().url());
    if (url.pathname.endsWith("/responses")) return route.fulfill({ json: responses });
    return route.fulfill({
      json: {
        id: SEARCH_ID,
        searchText: "album",
        token: 1,
        state: "Completed, TimedOut",
        startedAt: "2026-10-26T10:00:00Z",
        endedAt: "2026-10-26T10:00:15Z",
        responseCount: 2,
        fileCount: 5,
        lockedFileCount: 0,
        responseBytes: 0,
        responses: null,
        scope: { type: "network", subjects: [] }
      } satisfies Schemas["Search"]
    });
  });
};

/** `count` finished transfers with three users, as a report sums them */
export const summary = (count: number) =>
  ({
    count,
    totalBytes: count * 40_000_000,
    distinctUsers: 3,
    averageSpeed: 900_000,
    averageWait: 4,
    averageDuration: 30
  }) satisfies Schemas["TransferSummary"];

/**
 * Answers the dashboard's reports with a week of transfers, a leaderboard with a long name, a
 * long folder and a long error, the widths a narrow screen has to hold
 */
export const fakeReports = async (page: Page) => {
  const long = "a_rather_long_username_from_somewhere";
  await page.route("**/api/v0/telemetry/reports/transfers/**", (route) => {
    const url = new URL(route.request().url());
    const path = url.pathname.split("/").pop();
    const direction = url.searchParams.get("direction") === "Upload" ? "Upload" : "Download";
    if (path === "summary") {
      return route.fulfill({
        json: {
          Download: { Succeeded: summary(40), Errored: summary(3) },
          Upload: { Succeeded: summary(25), TimedOut: summary(1) },
          distinctUsers: 9
        } satisfies Schemas["SummaryReport"]
      });
    }
    if (path === "histogram") {
      return route.fulfill({
        json: Object.fromEntries(
          Array.from({ length: 24 }, (_, n) => [
            new Date(Date.UTC(2026, 9, 27, n)).toISOString(),
            {
              Download: { Succeeded: summary(n), Errored: summary(n % 3) },
              Upload: { Succeeded: summary(24 - n) }
            } satisfies Schemas["DirectionSummaries"]
          ])
        )
      });
    }
    if (path === "leaderboard") {
      return route.fulfill({
        json: [long, "alice", "bob"].map(
          (username, n) =>
            ({
              username,
              ...summary(30 - n),
              // the first row's figures are the widest the formatters write
              ...(n === 0
                ? { totalBytes: 1023.99 * 1024 ** 3, averageSpeed: 1023.9 * 1024 ** 2 }
                : {})
            }) satisfies Schemas["LeaderboardEntry"]
        )
      });
    }
    if (path === "directories") {
      return route.fulfill({
        json: [
          {
            directory: "@@a\\Music\\Some Artist\\A Very Long Album Title (Deluxe Edition)",
            count: 12,
            distinctUsers: 4
          }
        ] satisfies Schemas["DirectoryCount"][]
      });
    }
    const error = "Remote connection closed before the transfer finished, and it was not retried";
    if (path === "pareto") {
      return route.fulfill({
        json: [
          { exception: error, count: 4, distinctUsers: 2 }
        ] satisfies Schemas["ExceptionCount"][]
      });
    }
    return route.fulfill({
      json: [
        {
          id:
            direction === "Upload"
              ? "0b3c6f9e-1d1e-4c43-9d55-3a1f0b6e7a11"
              : "0b3c6f9e-1d1e-4c43-9d55-3a1f0b6e7a10",
          username: long,
          direction,
          filename: "@@a\\Music\\Some Artist\\A Very Long Album Title\\01 A Long Track Name.flac",
          size: 40_000_000,
          bytesTransferred: 1_000_000,
          state: "Completed, Errored",
          requestedAt: at(0),
          enqueuedAt: at(1),
          startedAt: at(2),
          endedAt: at(3),
          averageSpeed: 100_000,
          exception: error
        }
      ] satisfies Schemas["TransferExceptionDetail"][]
    });
  });
};

/** A folder of a browse, with `files` in it */
export const dir = (name: string, files: string[]) =>
  ({
    name,
    fileCount: files.length,
    files: files.map((f) => file(f))
  }) satisfies Schemas["Directory"];

/**
 * Answers a browse of carol's shares with `directories`, never asking the daemon, so no other test
 * sees her. Her profile is left to the daemon, which has not heard of her
 */
export const carolShares = (
  page: Page,
  directories: Schemas["Directory"][],
  lockedDirectories: Schemas["Directory"][] = []
) =>
  page.route(
    (url) => url.pathname.includes("/api/v0/users/carol/"),
    (route) => {
      const { pathname } = new URL(route.request().url());
      if (pathname.endsWith("/carol/profile")) return route.fallback();
      if (!pathname.endsWith("/carol/browse")) {
        return route.fulfill({ status: 404, json: { error: "not found" } });
      }
      return route.fulfill({
        json: {
          directories,
          directoryCount: directories.length,
          lockedDirectories,
          lockedDirectoryCount: lockedDirectories.length
        } satisfies Schemas["BrowseResponse"]
      });
    }
  );

/** Answers a browse of carol's shares with three folders */
export const fakeBrowse = (page: Page) =>
  carolShares(page, [
    dir("@@c\\Music", []),
    dir("@@c\\Music\\Albums", ["1.flac"]),
    dir("@@c\\Music\\Bootlegs", ["2.flac"])
  ]);

/** Answers alice's whole folder, asked from a search result, with its files and a subfolder */
export const fakeFolder = async (page: Page) => {
  await page.route("**/api/v0/users/alice/directory**", (route) =>
    route.fulfill({
      json: [
        dir("@@a\\Music\\Album", ["01 One.flac", "02 Two.flac", "04 A Long Track Name.flac"]),
        dir("@@a\\Music\\Album\\Bonus", ["05 Demo.flac"])
      ]
    })
  );
};

/** When the files in a Files listing were made and changed */
const LISTED_AT = "2026-10-27T18:00:00Z";

/** A file in a Files listing, `fullName` as the listing writes it */
export const listedFile = (fullName: string, length = 30_000_000) =>
  ({
    attributes: "Normal",
    createdAt: LISTED_AT,
    modifiedAt: LISTED_AT,
    fullName,
    name: fullName.slice(fullName.lastIndexOf("/") + 1),
    length
  }) satisfies Schemas["FilesystemFile"];

/** A folder in a Files listing, its entries unread unless `rest` holds them */
export const listedFolder = (
  fullName: string,
  name: string,
  rest: Pick<Schemas["FilesystemDirectory"], "directories" | "files"> = {
    directories: null,
    files: null
  }
) =>
  ({
    attributes: "Directory",
    createdAt: LISTED_AT,
    modifiedAt: LISTED_AT,
    fullName,
    name,
    ...rest
  }) satisfies Schemas["FilesystemDirectory"];

/** Answers the downloads folder's listing on the Files screen with two folders and a file */
export const fakeFiles = async (page: Page) => {
  const folder = (name: string) => listedFolder(`/downloads/${name}`, name);
  await page.route("**/api/v0/files/downloads/directories", (route) =>
    route.fulfill({
      json: listedFolder("/downloads/", "", {
        directories: [folder("In Rainbows"), folder("Kid A (Collector's Edition)")],
        files: [listedFile("/downloads/notes.txt", 1_234_567)]
      })
    })
  );
};

/** What the daemon knows of `username`, someone we transferred with and messaged once */
export const user = (username: string, rest: Partial<Schemas["UserSummary"]> = {}) => {
  const counts = { succeeded: 2, failed: 0, active: 0, queued: 0, bytes: 0 };
  return {
    username,
    known: true,
    firstSeenAt: "2026-10-01T10:00:00Z",
    lastSeenAt: "2026-10-27T10:00:00Z",
    presence: "Offline",
    presenceAt: "2026-10-27T09:00:00Z",
    isPrivileged: false,
    statistics: null,
    info: null,
    countryCode: "NL",
    groups: [{ name: "jazz", kind: "manual", defined: true, uploadSettings: false, color: "teal" }],
    uploads: { queue: "default", slots: 10, speed: "default", speedLimit: null, limits: [] },
    block: null,
    notes: null,
    description: null,
    muted: false,
    buddy: null,
    conversation: null,
    endpoint: null,
    figures: { downloads: counts, uploads: counts, messages: { in: 1, out: 1, unread: 0 } },
    unreadMessages: 0,
    activeTransfers: 0,
    ...rest
  } satisfies Schemas["UserSummary"];
};

/** A download of `filename` from `username`, finished unless `state` says otherwise */
export const transfer = (
  filename: string,
  state: Schemas["TransferStates"] = "Completed, Succeeded",
  size = 1000,
  username = "alice"
) =>
  ({
    id: newId(),
    username,
    direction: "Download",
    filename,
    size,
    state,
    requestedAt: "2026-10-26T10:00:00Z",
    bytesTransferred: state === "InProgress" ? size * 0.4 : size,
    averageSpeed: state === "InProgress" ? 100 : 0,
    percentComplete: state === "InProgress" ? 40 : 100,
    attempts: 1,
    paused: false,
    removed: false
  }) satisfies Schemas["Transfer"];

/** `username`'s downloads in one remote folder, `directory`, as the list of downloads groups them */
export const userDownloads = (username: string, files: Schemas["Transfer"][], directory = "d") =>
  ({
    username,
    directories: [{ directory, fileCount: files.length, files }]
  }) satisfies Schemas["UserResponse"];

/** Answers the list of downloads with `users`, read again on every call */
export const fakeDownloadList = (page: Page, users: () => Schemas["UserResponse"][]) =>
  page.route(
    (url) => url.pathname.endsWith("/api/v0/transfers/downloads"),
    (route) => route.fulfill({ json: users() })
  );

/** The finished download of alice's 2.flac, which `fakeAnalysis` has a report of */
export const ANALYZED_DOWNLOAD = "2f1ac000-0000-4000-8000-000000000002";

/**
 * Answers the downloads list with alice's two folders, one download running and one done, and
 * one of bob's as large as a size gets
 */
export const fakeDownloads = async (page: Page) => {
  const files = [
    transfer("@@a\\Music\\One\\1.flac", "InProgress"),
    {
      ...transfer("@@a\\Music\\Two\\2.flac"),
      id: ANALYZED_DOWNLOAD,
      localPath: "/downloads/alice/Two/2.flac"
    }
  ];
  const large = [transfer("@@b\\Big\\3.flac", "Completed, Succeeded", 1023.99 * 1024 ** 3, "bob")];
  await fakeDownloadList(page, () => [
    userDownloads("alice", files),
    userDownloads("bob", large, "b")
  ]);
};

/** `n` downloads from users of a hundred files each, ten to a folder, every one finished */
export const manyDownloads = async (page: Page, n: number) => {
  const users = Array.from({ length: Math.ceil(n / 100) }, (_, u) => {
    const username = `user${String(u).padStart(3, "0")}`;
    return {
      username,
      directories: Array.from({ length: 10 }, (_, d) => ({
        directory: `Album ${d}`,
        fileCount: 10,
        files: Array.from({ length: 10 }, (_, f) => {
          const k = u * 100 + d * 10 + f;
          return {
            ...transfer(
              `@@u${u}\\Music\\Album ${d}\\${String(k).padStart(5, "0")} Track.flac`,
              "Completed, Succeeded",
              1000,
              username
            ),
            id: `00000000-0000-4000-8000-${String(k).padStart(12, "0")}`
          };
        })
      }))
    } satisfies Schemas["UserResponse"];
  });
  await fakeDownloadList(page, () => users);
};

/** A browse of carol's shares with `n` folders, fifty to an artist */
export const wideBrowse = async (page: Page, n: number) => {
  const directories = [dir("@@c\\Music", [])];
  for (let a = 0; directories.length < n; a++) {
    directories.push(dir(`@@c\\Music\\Artist ${String(a).padStart(3, "0")}`, []));
    for (let b = 0; b < 49 && directories.length < n; b++) {
      directories.push(
        dir(
          `@@c\\Music\\Artist ${String(a).padStart(3, "0")}\\Album ${String(b).padStart(2, "0")}`,
          ["1.flac"]
        )
      );
    }
  }
  await carolShares(page, directories);
};

/** Room "indie" with `n` messages in its history, all in one page */
export const longRoom = async (page: Page, n: number) => {
  const messages = Array.from({ length: n }, (_, k) =>
    roomMessage(
      k + 1,
      new Date(Date.UTC(2026, 9, 27, 10) + k * 37_000).toISOString(),
      MEMBERS[k % 5],
      k % 9 === 0 ? `a longer message, number ${k}, `.repeat(6) : `message number ${k}`
    )
  );
  const history = { messages, hasMore: false, gaps: [] };
  await page.route("**/api/v0/rooms/joined", (route) =>
    route.fulfill({
      json: [
        joinedRoom("indie", {
          userCount: 5,
          lastMessageAt: messages.at(-1)!.timestamp,
          lastMessage: messages.at(-1),
          lastReadId: n
        })
      ]
    })
  );
  await page.route("**/api/v0/rooms/history/indie/messages**", (route) =>
    route.fulfill({ json: history })
  );
  await page.route("**/api/v0/rooms/joined/indie**", (route) =>
    new URL(route.request().url()).pathname.endsWith("/messages")
      ? route.fulfill({ json: history })
      : route.fulfill({
          json: {
            name: "indie",
            isPrivate: false,
            users: [],
            tickers: [],
            messages: null
          } satisfies Schemas["Room"]
        })
  );
  await page.route("**/api/v0/conversations**", (route) => route.fulfill({ json: [] }));
};

/** A report of a 128 kbps transcode `duration` seconds long, with the judgment `override` */
const report = (
  override: Schemas["AnalysisOverride"] | null,
  file: { relativePath: string; transferId: string | null } = {
    relativePath: "alice/Two/2.flac",
    transferId: ANALYZED_DOWNLOAD
  },
  scale: Schemas["SpectrogramScale"] = "linear",
  duration = 30.5
) =>
  ({
    path: `/downloads/${file.relativePath}`,
    root: "downloads",
    ...file,
    size: 1000,
    analyzedAt: "2026-10-28T12:00:00Z",
    durationSeconds: duration,
    claimed: { codec: "flac", sampleRate: 44100, bitDepth: 16, channels: 2, bitrateKbps: null },
    verdict: "likely_transcode",
    confidence: 0.89,
    flags: [],
    encoderGuess: { profile: "mp3 ~128 kbps", confidence: 0.95 },
    summary:
      "100% of the time stops at 16.8 kHz with a sharp shelf, where an encoder at mp3 ~128 kbps cuts off.",
    measures: {
      cutoffHzHistogram: [[17000, 30]],
      cutoffMedianHz: 16774,
      shelfSharpnessDbP95: 61.8,
      sfb21Holes: null,
      frameArtifactScore: 0.02,
      upsampleRatio: null,
      bitDepthEffective: 16,
      noiseFloorDbfs: -25,
      stereoCorrelation: 0.6,
      sideEnergyDb: -6,
      dcOffset: 0,
      clippedSamplesPct: 0,
      peakDbfs: -0.4
    },
    windows: Array.from({ length: Math.floor(duration) }, (_, t) => ({
      t,
      cutoffHz: 16774,
      shelfDb: 61.8,
      rmsDbfs: -20
    })),
    spectrogramUrl: "/api/v0/files/downloads/files/x/analysis/spectrogram",
    spectrogram: layout(duration, scale),
    override,
    limitations: ["Of several lossy generations, only the lowest cutoff shows"],
    stale: false
  }) satisfies Schemas["AudioAnalysis"];

/**
 * Answers every file's and download's analysis with the report of a transcode `duration` seconds
 * long, its spectrogram, waveform and window spectra and an empty queue, and the list of reports
 * with 2.flac's and track.flac's; a judgment put is kept and read back. With `stale`, a file's
 * report is one an older version made
 */
export const fakeAnalysis = async (page: Page, { stale = false, duration = 30.5 } = {}) => {
  let override: Schemas["AnalysisOverride"] | null = null;
  await page.route(
    (url) => /\/analysis(\/|$)/.test(url.pathname),
    (route) => {
      const { pathname } = new URL(route.request().url());
      const method = route.request().method();
      if (pathname.endsWith("/analysis/jobs")) {
        return route.fulfill({ json: { jobs: [], backlog: 0 } satisfies Schemas["AnalysisQueue"] });
      }
      if (pathname.endsWith("/analysis/uploads")) {
        return route.fulfill({
          status: 201,
          json: {
            root: "temporary",
            path: "1f2e3d4c/mine.flac",
            size: 2048
          } satisfies Schemas["AnalysisUpload"]
        });
      }
      // the list the chips and filters read: the download's report, and one of a file in Files
      if (pathname.endsWith("/api/v0/analysis")) {
        return route.fulfill({
          json: [
            { ...report(override), windows: null },
            { ...report(null, { relativePath: "track.flac", transferId: null }), windows: null }
          ]
        });
      }
      const tile = new URL(route.request().url()).searchParams.get("tile");
      const piece = {
        tile: tile === null ? null : Number(tile),
        contentType: "application/octet-stream"
      };
      if (pathname.endsWith("/analysis/spectrogram")) {
        return route.fulfill({
          body: spectrogram(duration, piece.tile),
          contentType: piece.contentType
        });
      }
      if (pathname.endsWith("/analysis/waveform")) {
        return route.fulfill({
          body: waveform(duration, piece.tile),
          contentType: piece.contentType
        });
      }
      if (pathname.endsWith("/analysis/spectra")) {
        return route.fulfill({
          body: spectra(Math.floor(duration)),
          contentType: piece.contentType
        });
      }
      if (pathname.endsWith("/override")) {
        if (method === "PUT") {
          const body = route.request().postDataJSON() as Schemas["SetAnalysisOverride"];
          override = { ...body, by: "local:reseek", at: "2026-10-28T12:05:00Z" };
        } else {
          override = null;
        }
        return route.fulfill({ status: 204 });
      }
      if (method === "POST") {
        return route.fulfill({
          status: 409,
          json: { error: "The file has already been analyzed" }
        });
      }
      const json = report(override, undefined, "linear", duration);
      // an older version's report: the file, the reading and the judgment, nothing to draw
      if (stale)
        return route.fulfill({ json: { ...json, stale, spectrogram: null, windows: null } });
      return route.fulfill({ json });
    }
  );
};

/** Answers the batch analysis call: `each` says what became of each file it names, in order */
export const fakeBatch = async (
  page: Page,
  each: (file: Schemas["AnalysisTarget"]) => "queued" | "current" | "refused"
) => {
  await page.route("**/api/v0/analysis/batch", (route) => {
    const { files } = route.request().postDataJSON() as Schemas["AnalysisBatch"];
    const result: Schemas["AnalysisBatchQueued"] = {
      queued: 0,
      current: 0,
      refused: [],
      jobId: null
    };
    files.forEach((file, index) => {
      const outcome = each(file);
      if (outcome === "refused") result.refused.push({ index, error: "The file is gone" });
      else result[outcome]++;
    });
    return route.fulfill({ status: 202, json: result });
  });
};
