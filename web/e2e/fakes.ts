import type { Page } from "@playwright/test";

/** A time on the evening the fakes take place */
export const at = (minute: number, second = 0) =>
  new Date(Date.UTC(2026, 9, 27, 18, minute, second)).toISOString();

const MEMBERS = ["alice", "bob", "carol", "dave", "erin", "frank", "grace", "heidi"];

/** Answers the chat calls with one room, "indie", and one conversation, with "alice" */
export const fakeChat = async (page: Page) => {
  const said: [string, string][] = [
    ["alice", "anyone have the new boards of canada rip?"],
    ["bob", "yes, slsk://bob/Music/Boards%20of%20Canada/Tomorrow's%20Harvest/ in flac"],
    ["bob", "320 too if you want it smaller"],
    ["carol", "/me waves at everyone"],
    ["reseek", "thanks bob, grabbing it now"],
    ["dave", "reseek: did you see https://example.com/review ?"]
  ];
  const roomMessages = said.map(([username, message], n) => ({
    localId: n + 1,
    timestamp: at(n),
    username,
    message,
    roomName: "indie",
    self: username === "reseek",
    isMention: message.includes("reseek"),
    hidden: false
  }));
  const joined = [
    {
      name: "indie",
      unread: 0,
      mentions: 0,
      notify: "mentions",
      userCount: MEMBERS.length,
      lastMessageAt: at(5),
      lastReadId: 6
    }
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
          { username: "alice", ticker: "listening to Geogaddi" },
          { username: "bob", ticker: "sharing the whole BoC catalogue" }
        ],
        messages: null
      }
    });
  });
  const conversation = {
    username: "alice",
    isActive: true,
    isServerNotice: false,
    unAcknowledgedMessageCount: 0,
    hasUnAcknowledgedMessages: false,
    notify: "all",
    draft: "",
    lastMessage: null,
    messages: null
  };
  await page.route("**/api/v0/conversations**", (route) => {
    const url = new URL(route.request().url());
    if (url.pathname.endsWith("/api/v0/conversations"))
      return route.fulfill({ json: [conversation] });
    if (url.pathname.endsWith("/messages")) {
      return route.fulfill({
        json: [
          ["In", "hey, are you still sharing the warp records stuff?"],
          ["Out", "yes, browse away"],
          ["In", "thanks!"]
        ].map(([direction, message], n) => ({
          localId: n + 1,
          id: n + 1,
          timestamp: at(10 + n),
          username: "alice",
          direction,
          message,
          isAcknowledged: true,
          wasReplayed: false
        }))
      });
    }
    return route.fulfill({ json: conversation });
  });
};

/** The id of the finished search `fakeSearch` answers for */
export const SEARCH_ID = "6f1c9a52-0d1e-4c43-9d55-3a1f0b6e7a10";

/** A shared file as a search response or a browse lists it */
export const file = (filename: string, size = 4_000_000) => ({
  filename,
  size,
  code: 1,
  extension: filename.split(".").pop(),
  isLocked: false,
  attributeCount: 0,
  attributes: [],
  bitRate: 320,
  length: 200
});

const response = (username: string, speed: number, files: string[], size?: number) => ({
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
});

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
        isComplete: true,
        startedAt: "2026-10-26T10:00:00Z",
        endedAt: "2026-10-26T10:00:15Z",
        responseCount: 2,
        fileCount: 5,
        lockedFileCount: 0,
        responseBytes: 0,
        responses: null,
        scope: { type: "network", subjects: [] }
      }
    });
  });
};

const summary = (count: number) => ({
  count,
  totalBytes: count * 40_000_000,
  distinctUsers: 3,
  averageSpeed: 900_000,
  averageWait: 4,
  averageDuration: 30
});

/**
 * Answers the dashboard's reports with a week of transfers, a leaderboard with a long name, a
 * long folder and a long error, the widths a narrow screen has to hold
 */
export const fakeReports = async (page: Page) => {
  const long = "a_rather_long_username_from_somewhere";
  await page.route("**/api/v0/telemetry/reports/transfers/**", (route) => {
    const url = new URL(route.request().url());
    const path = url.pathname.split("/").pop();
    const direction = url.searchParams.get("direction") ?? "Download";
    if (path === "summary") {
      return route.fulfill({
        json: {
          Download: { Succeeded: summary(40), Errored: summary(3) },
          Upload: { Succeeded: summary(25), TimedOut: summary(1) },
          distinctUsers: 9
        }
      });
    }
    if (path === "histogram") {
      return route.fulfill({
        json: Object.fromEntries(
          Array.from({ length: 24 }, (_, n) => [
            new Date(Date.UTC(2026, 9, 27, n)).toISOString(),
            { Download: { Succeeded: summary(n) }, Upload: { Succeeded: summary(24 - n) } }
          ])
        )
      });
    }
    if (path === "leaderboard") {
      return route.fulfill({
        json: [long, "alice", "bob"].map((username, n) => ({
          username,
          ...summary(30 - n),
          // the first row's figures are the widest the formatters write
          ...(n === 0 ? { totalBytes: 1023.99 * 1024 ** 3, averageSpeed: 1023.9 * 1024 ** 2 } : {})
        }))
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
        ]
      });
    }
    const error = "Remote connection closed before the transfer finished, and it was not retried";
    if (path === "pareto") {
      return route.fulfill({
        json: [{ exception: error, count: 4, distinctUsers: 2 }]
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
      ]
    });
  });
};

const dir = (name: string, files: string[]) => ({
  name,
  fileCount: files.length,
  files: files.map((f) => file(f))
});

/**
 * Answers a browse of carol's shares with three folders, never asking the daemon, so no other
 * test sees her
 */
export const fakeBrowse = async (page: Page) => {
  await page.route(
    (url) => url.pathname.includes("/api/v0/users/carol/"),
    (route) =>
      !new URL(route.request().url()).pathname.endsWith("/carol/browse")
        ? route.fulfill({ status: 404, json: { error: "not found" } })
        : route.fulfill({
            json: {
              directories: [
                dir("@@c\\Music", []),
                dir("@@c\\Music\\Albums", ["1.flac"]),
                dir("@@c\\Music\\Bootlegs", ["2.flac"])
              ],
              directoryCount: 3,
              lockedDirectories: [],
              lockedDirectoryCount: 0
            }
          })
  );
};

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

/** Answers the downloads folder's listing on the Files screen with two folders and a file */
export const fakeFiles = async (page: Page) => {
  const at = "2026-10-27T18:00:00Z";
  const folder = (name: string) => ({
    attributes: "Directory",
    createdAt: at,
    modifiedAt: at,
    fullName: `/downloads/${name}`,
    name,
    directories: null,
    files: null
  });
  await page.route("**/api/v0/files/downloads/directories", (route) =>
    route.fulfill({
      json: {
        ...folder(""),
        directories: [folder("In Rainbows"), folder("Kid A (Collector's Edition)")],
        files: [
          {
            attributes: "Normal",
            createdAt: at,
            modifiedAt: at,
            fullName: "/downloads/notes.txt",
            name: "notes.txt",
            length: 1_234_567
          }
        ]
      }
    })
  );
};
