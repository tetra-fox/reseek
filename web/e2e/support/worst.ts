import type { Page } from "@playwright/test";

import type { Schemas } from "../../src/lib/api/client";
import {
  conversation,
  file,
  joinedRoom,
  privateMessage,
  response,
  roomMessage,
  SEARCH_ID,
  sharesOf,
  transfer,
  user,
  userDownloads
} from "./fakes";

/**
 * Usernames as the network has them at their worst: one letter, longer than a column, spaces at
 * the ends, emoji with a joiner, right-to-left, a zero-width space, and no spaces at all in a
 * script that breaks anywhere
 */
export const NAMES = {
  one: "j",
  long: "xX_DJ_Bartholomew_Fitzgerald_Xx_1999",
  spaced: " dj shadow ",
  emoji: "🎧 vinyl 👩🏽‍💻",
  rtl: "نور الهدى",
  hidden: "alice​",
  cjk: "王秀英的音乐收藏"
} as const;

const ALL = Object.values(NAMES);

/** The largest a 32-bit field a peer fills in can hold */
const U32 = 4_294_967_295;

/** A folder path of about 400 characters with no spaces, as a box set's rip names them */
export const LONG_FOLDER =
  "@@w\\Music\\Various_Artists\\VA_-_The_Ultimate_Collection_of_Progressive_Psytrance_and_" +
  "Full-On_Goa_Trance_Classics_Vol.1-12_(1996-2008)_[WEB-FLAC-24bit-96kHz]_{Remastered_" +
  "Deluxe_Box_Set_Edition_with_Bonus_Material}\\CD07_-_Night_Sessions_Continuous_Mix_by_" +
  "Various_Artists_Compiled_and_Mixed_by_Someone_Famous_(Unmixed_Versions_Included)";

/** A file name of about 150 characters with no spaces; the two differ only at the end */
export const LONG_FILE =
  "07-13_-_Artist_Name_feat._Another_Artist_and_Friends_-_A_Really_Long_Track_Title_" +
  "That_Goes_On_(Extended_Original_Mix_Remastered_2008)_part1.flac";
export const LONG_FILE_2 = LONG_FILE.replace("part1", "part2");

/** A file with the attributes a peer sent, `rest` over a plain one */
const shared = (filename: string, size: number, rest: Partial<Schemas["File"]> = {}) => ({
  ...file(filename, size),
  ...rest
});

/** One line of a megabyte, as a profile description pasted from somewhere */
export const HUGE_LINE = "x".repeat(1_000_000);

/**
 * The files of the worst search: paths at their longest, sizes of nothing and 200 GB, bitrates,
 * lengths and sample rates of nothing and the most a field holds
 */
const searchFiles = (n: number) => [
  shared(`${LONG_FOLDER}\\${LONG_FILE}`, 0, { bitRate: 0, length: 0 }),
  shared(`${LONG_FOLDER}\\${LONG_FILE_2}`, 200 * 1024 ** 3, { bitRate: U32, length: U32 }),
  shared(`@@w\\مكتبة\\موسيقى_عربية\\${n}_أغنية_طويلة_جدا.mp3`, 9_000_000, {
    bitRate: 320,
    length: 245,
    isVariableBitRate: true
  }),
  shared(`@@w\\音楽\\坂本龍一\\${n}_戦場のメリークリスマス.flac`, 41_000_000, {
    bitDepth: U32,
    sampleRate: U32,
    length: 300
  })
];

/** Answers the worst search: a term in two scripts, a response from each name, counts in millions */
export const worstSearch = async (page: Page) => {
  const responses = ALL.map((name, n) => ({
    ...response(name, n === 0 ? 0 : U32, []),
    files: searchFiles(n),
    fileCount: 4,
    queueLength: n === 1 ? U32 : n,
    hasFreeUploadSlot: n % 2 === 0,
    country: n === 2 ? null : "JP"
  }));
  const search = {
    id: SEARCH_ID,
    searchText: "坂本龍一 戦場のメリークリスマス موسيقى عربية Ultimate_Collection_of_Progressive",
    token: 1,
    state: "Completed, TimedOut",
    startedAt: "2026-10-26T10:00:00Z",
    endedAt: "2026-10-26T10:00:15Z",
    responseCount: 2_400_000,
    fileCount: 9_999_999,
    lockedFileCount: 1_234_567,
    responseBytes: 200 * 1024 ** 3,
    responses: null,
    scope: { type: "network", subjects: [] }
  } satisfies Schemas["Search"];
  await page.route("**/api/v0/searches", (route) =>
    route.request().method() === "GET" ? route.fulfill({ json: [search] }) : route.fallback()
  );
  await page.route(`**/api/v0/searches/${SEARCH_ID}**`, (route) => {
    const url = new URL(route.request().url());
    if (url.pathname.endsWith("/responses")) return route.fulfill({ json: responses });
    return route.fulfill({ json: search });
  });
};

/** A browse of `NAMES.long`'s shares: a long folder of 5000 files, and folders in two scripts */
export const worstBrowse = (page: Page) => {
  const big = {
    name: LONG_FOLDER,
    fileCount: 5000,
    files: Array.from({ length: 5000 }, (_, k) =>
      shared(
        k === 0 ? LONG_FILE : k === 1 ? LONG_FILE_2 : `${String(k).padStart(4, "0")} Track.flac`,
        k % 2 ? 200 * 1024 ** 3 : 0,
        { bitRate: k % 3 ? U32 : 0, length: k % 3 ? U32 : 0 }
      )
    )
  };
  const parents = ["@@w\\Music", "@@w\\Music\\Various_Artists"].map((name) => ({
    name,
    fileCount: 0,
    files: []
  }));
  const scripts = ["@@w\\مكتبة\\موسيقى_عربية", "@@w\\音楽\\坂本龍一"].map((name, n) => ({
    name,
    fileCount: 1,
    files: [searchFiles(n)[2 + n]]
  }));
  return sharesOf(page, NAMES.long, [...parents, big, ...scripts]);
};

/** A transfer of `filename` from `username` in `state`, `rest` over it */
const moved = (
  filename: string,
  username: string,
  state: Schemas["TransferStates"],
  size: number,
  rest: Partial<Schemas["Transfer"]> & { direction: Schemas["TransferDirection"] }
) => ({ ...transfer(filename, state, size, username, rest.direction), ...rest });

/** One line of an error as a peer's reason can make it */
const LONG_ERROR =
  "Transfer rejected: " + "File_not_shared_or_access_denied_by_peer_policy_".repeat(8).trim();

/** The worst downloads: every name, the longest paths, a size of nothing and of 200 GB */
const worstTransfers = (direction: Schemas["TransferDirection"]) =>
  ALL.map((name, n) =>
    userDownloads(
      name,
      [
        moved(`${LONG_FOLDER}\\${LONG_FILE}`, name, "InProgress", 200 * 1024 ** 3, {
          direction,
          averageSpeed: U32,
          bytesTransferred: 100 * 1024 ** 3,
          percentComplete: 50,
          remainingTime: U32
        }),
        moved(`${LONG_FOLDER}\\${LONG_FILE_2}`, name, "Queued, Remotely", 0, {
          direction,
          bytesTransferred: 0,
          percentComplete: 0,
          placeInQueue: U32
        }),
        moved(`@@w\\مكتبة\\موسيقى_عربية\\${n}_أغنية.mp3`, name, "Completed, Errored", 9_000_000, {
          direction,
          exception: LONG_ERROR
        })
      ],
      LONG_FOLDER
    )
  );

/** Answers the downloads and the uploads lists with the worst transfers */
export const worstTransferLists = async (page: Page) => {
  const downloads = worstTransfers("Download");
  const uploads = worstTransfers("Upload");
  await page.route(
    (url) => url.pathname.endsWith("/api/v0/transfers/downloads"),
    (route) => route.fulfill({ json: downloads })
  );
  await page.route(
    (url) => url.pathname.endsWith("/api/v0/transfers/uploads"),
    (route) => route.fulfill({ json: uploads })
  );
};

/** Our transfers with the worst user, the bytes as a profile has them and a summary leaves out */
const DOWNLOADS = { succeeded: 9_999_999, failed: 1_234_567, active: 0, queued: 9_999, bytes: 0 };
const UPLOADS = { succeeded: 2_400_000, failed: 0, active: 3, queued: 0, bytes: 200 * 1024 ** 4 };

/**
 * `name` as the daemon knows them: a description of a megabyte, groups and notes at length, and
 * figures in the millions. `NAMES.long` has a picture forty times as wide as it is tall
 */
export const worstUser = (name: string = NAMES.long) =>
  user(name, {
    presence: "Online",
    countryCode: "JP",
    description: HUGE_LINE,
    notes: `${"Met in the psytrance room, shares rare Goa 12-inch rips, ask before queueing more than one box set. ".repeat(20)}${"y".repeat(300)}`,
    statistics: {
      averageSpeed: U32,
      uploadCount: U32,
      fileCount: 9_999_999,
      directoryCount: 2_400_000,
      fetchedAt: "2026-10-27T09:00:00Z",
      stale: false
    },
    info: {
      fetchedAt: "2026-10-27T09:00:00Z",
      hasFreeUploadSlot: false,
      hasPicture: name === NAMES.long,
      pictureUrl: name === NAMES.long ? PICTURE : null,
      queueLength: U32,
      stale: false,
      uploadSlots: U32
    },
    groups: [
      "jazz",
      "Progressive_Psytrance_and_Full-On_Goa_Trance_Collectors",
      "موسيقى",
      "音楽仲間"
    ].map((group) => ({
      name: group,
      kind: "manual",
      defined: true,
      uploadSettings: false,
      color: "teal"
    })),
    figures: {
      downloads: DOWNLOADS,
      uploads: UPLOADS,
      messages: { in: 1_000_000, out: 0, unread: 1_000_000 }
    },
    unreadMessages: 1_000_000,
    activeTransfers: 3
  });

/** Where the worst user's picture is served, a panorama forty times as wide as it is tall */
const PICTURE = "/api/v0/users/worst/picture";

/**
 * Answers every call about `name` with the worst user: the summary, and the profile with the
 * description of a megabyte and the panorama for a picture
 */
export const worstProfile = async (page: Page, name: string = NAMES.long) => {
  await page.route(`**${PICTURE}`, (route) =>
    route.fulfill({
      contentType: "image/svg+xml",
      body: '<svg xmlns="http://www.w3.org/2000/svg" width="4000" height="100"><rect width="4000" height="100" fill="#c0392b"/><rect width="100" height="100" fill="#2980b9"/></svg>'
    })
  );
  await page.route(
    (url) => url.pathname.includes(`/api/v0/users/${encodeURIComponent(name)}/`),
    (route) => {
      const { pathname } = new URL(route.request().url());
      if (pathname.endsWith("/browse")) return route.fallback();
      if (pathname.endsWith("/interests")) {
        return route.fulfill({ json: { liked: [], hated: [] } });
      }
      const summary = worstUser(name);
      if (pathname.endsWith("/profile")) {
        return route.fulfill({
          json: {
            ...summary,
            notes: summary.notes!,
            info: { ...summary.info!, description: summary.description! },
            figures: summary.figures
          }
        });
      }
      return route.fulfill({ json: summary });
    }
  );
};

/** A room name in Arabic and one in Japanese, each long */
export const ROOMS = {
  rtl: "موسيقى عربية كلاسيكية",
  cjk: "日本の音楽とアニメのサウンドトラック"
} as const;

/** A link of a thousand characters with nowhere to break, as pasted from a share */
const LONG_LINK = `https://example.com/share/${"a1b2c3d4e5".repeat(100)}`;

/** Answers the chat calls with the worst room and the worst conversation */
export const worstChat = async (page: Page) => {
  const said: [string, string][] = [
    [NAMES.one, "hi"],
    [NAMES.long, LONG_LINK],
    [NAMES.rtl, "مرحبا بالجميع، هل لدى أحد ألبوم فيروز الكامل بجودة عالية؟"],
    [NAMES.cjk, "坂本龍一の戦場のメリークリスマスを探しています。".repeat(8)],
    [NAMES.emoji, "🎧🎧🎧 " + "🔥".repeat(200)],
    [NAMES.spaced, `${"z".repeat(600)}`],
    [NAMES.hidden, "same name as alice, different user"]
  ];
  const messages = said.map(([username, message], n) =>
    roomMessage(n + 1, `2026-10-27T18:0${n}:00Z`, username, message, ROOMS.rtl)
  );
  const room = {
    name: ROOMS.rtl,
    isPrivate: false,
    users: ALL.map((username, n) => ({
      username,
      status: n % 3 === 2 ? "Away" : "Online",
      averageSpeed: n ? U32 : 0,
      uploadCount: U32,
      fileCount: 9_999_999,
      directoryCount: 2_400_000,
      countryCode: n === 3 ? null : "JP",
      slotsFull: n % 2 === 1,
      self: false
    })),
    tickers: [
      { username: NAMES.long, ticker: `listening to ${LONG_FILE}`, hidden: false },
      { username: NAMES.rtl, ticker: "أستمع إلى فيروز طوال اليوم", hidden: false },
      { username: NAMES.cjk, ticker: "坂本龍一".repeat(30), hidden: false }
    ],
    messages: null
  } satisfies Schemas["Room"];
  const history = { messages, hasMore: false, gaps: [] };
  const joined = [
    joinedRoom(ROOMS.rtl, {
      userCount: 2_400_000,
      unread: 1_000_000,
      mentions: 9_999,
      lastMessageAt: messages.at(-1)!.timestamp,
      lastMessage: messages.at(-1),
      lastReadId: 1
    }),
    joinedRoom(ROOMS.cjk, { userCount: 1 })
  ];
  await page.route("**/api/v0/rooms/joined", (route) => route.fulfill({ json: joined }));
  await page.route("**/api/v0/rooms/history/**", (route) => route.fulfill({ json: history }));
  await page.route("**/api/v0/rooms/joined/*/**", (route) =>
    new URL(route.request().url()).pathname.endsWith("/messages")
      ? route.fulfill({ json: history })
      : route.fulfill({ json: room })
  );
  await page.route("**/api/v0/rooms/joined/*", (route) =>
    route.request().method() === "GET" ? route.fulfill({ json: room }) : route.fallback()
  );
  const pm = (["In", "Out", "In"] as const).map((direction, n) =>
    privateMessage(n + 1, `2026-10-27T18:1${n}:00Z`, NAMES.long, direction, said[n + 1][1])
  );
  const talks = ALL.map((name, n) =>
    conversation(name, {
      lastMessage: n === 1 ? pm.at(-1) : null,
      unAcknowledgedMessageCount: n === 1 ? 1_000_000 : 0,
      hasUnAcknowledgedMessages: n === 1
    })
  );
  await page.route("**/api/v0/conversations**", (route) => {
    const { pathname } = new URL(route.request().url());
    if (pathname.endsWith("/api/v0/conversations")) return route.fulfill({ json: talks });
    if (pathname.endsWith("/messages")) return route.fulfill({ json: pm });
    return route.fulfill({ json: talks[1] });
  });
};

/** Answers the directory of users with one of each name, the worst user's figures on each */
export const worstDirectory = (page: Page) =>
  page.route("**/api/v0/users?**", (route) => {
    const users = ALL.map((name) => worstUser(name));
    return route.fulfill({ json: { users, total: 2_400_000 } });
  });
