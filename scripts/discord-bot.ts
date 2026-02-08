#!/usr/bin/env -S npx tsx
/**
 * HomeGPT Discord Bot
 *
 * Full two-way Discord bot enabling HomeGPT to communicate with the family.
 * Handles both proactive messaging (heartbeat alerts, morning briefings)
 * and reactive messaging (users @mention or DM the bot).
 *
 * Usage:
 *   npx tsx scripts/discord-bot.ts
 *
 * Environment:
 *   DISCORD_BOT_TOKEN  - Bot token from Discord Developer Portal
 *   DISCORD_GUILD_ID   - Server ID (for slash command registration)
 *   DISCORD_OWNER_IDS  - Comma-separated Discord user IDs (you + wife)
 *   HOMEGPT_URL        - HomeGPT API URL (default: http://localhost:31327)
 *   BOT_PORT           - Internal HTTP port for heartbeat integration (default: 31342)
 *
 * Channel routing (set channel IDs in env or auto-detect by name):
 *   DISCORD_CHANNEL_GENERAL    - General chat channel
 *   DISCORD_CHANNEL_ERGOTOOLS  - Business monitoring
 *   DISCORD_CHANNEL_SCHOOL     - Tutoring/curriculum updates
 *   DISCORD_CHANNEL_HOME       - Household management
 *   DISCORD_CHANNEL_CALENDAR   - Schedule/events
 */

import {
  Client,
  GatewayIntentBits,
  Partials,
  REST,
  Routes,
  EmbedBuilder,
  TextChannel,
  DMChannel,
  type Message,
  type Interaction,
  SlashCommandBuilder,
} from "discord.js";
import { createServer, IncomingMessage, ServerResponse } from "http";

const BOT_TOKEN = process.env.DISCORD_BOT_TOKEN || "";
const GUILD_ID = process.env.DISCORD_GUILD_ID || "";
const OWNER_IDS = (process.env.DISCORD_OWNER_IDS || "")
  .split(",")
  .map((id) => id.trim())
  .filter(Boolean);
const HOMEGPT_URL = process.env.HOMEGPT_URL || "http://localhost:31327";
const BOT_PORT = parseInt(process.env.BOT_PORT || "31342");

// Channel name → ID mapping (auto-detected on startup, or set via env)
const CHANNEL_MAP: Record<string, string> = {};
const CHANNEL_NAMES = [
  "general",
  "ergotools",
  "school",
  "home",
  "calendar",
] as const;

if (!BOT_TOKEN) {
  console.error("DISCORD_BOT_TOKEN is required");
  process.exit(1);
}

// --- Discord Client Setup ---

const client = new Client({
  intents: [
    GatewayIntentBits.Guilds,
    GatewayIntentBits.GuildMessages,
    GatewayIntentBits.DirectMessages,
    GatewayIntentBits.MessageContent,
  ],
  partials: [Partials.Channel, Partials.Message],
});

// --- HomeGPT API Communication ---

async function chatWithHomeGPT(
  message: string,
  userId?: string
): Promise<string> {
  try {
    const res = await fetch(`${HOMEGPT_URL}/api/chat`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        message,
        metadata: { source: "discord", user_id: userId },
      }),
    });

    if (!res.ok) {
      const text = await res.text();
      console.error(`HomeGPT error: ${res.status} ${text}`);
      return "Sorry, I'm having trouble connecting to my brain right now. Try again in a moment.";
    }

    const data = await res.json();
    return data.response || "I processed your message but have nothing to say.";
  } catch (e) {
    console.error("HomeGPT connection failed:", e);
    return "I can't reach my backend right now. Is HomeGPT running?";
  }
}

// --- Message Handling ---

// Split long messages for Discord's 2000 char limit
function splitMessage(text: string, maxLen = 1900): string[] {
  if (text.length <= maxLen) return [text];

  const chunks: string[] = [];
  let remaining = text;

  while (remaining.length > 0) {
    if (remaining.length <= maxLen) {
      chunks.push(remaining);
      break;
    }

    // Try to split at paragraph, then sentence, then word boundary
    let splitAt = remaining.lastIndexOf("\n\n", maxLen);
    if (splitAt < maxLen / 2) splitAt = remaining.lastIndexOf(". ", maxLen);
    if (splitAt < maxLen / 2) splitAt = remaining.lastIndexOf(" ", maxLen);
    if (splitAt < 1) splitAt = maxLen;

    chunks.push(remaining.slice(0, splitAt + 1));
    remaining = remaining.slice(splitAt + 1);
  }

  return chunks;
}

async function handleMessage(msg: Message) {
  // Ignore own messages
  if (msg.author.id === client.user?.id) return;
  // Ignore other bots
  if (msg.author.bot) return;

  const isDM = msg.channel instanceof DMChannel;
  const isMentioned =
    msg.mentions.has(client.user!.id) || msg.content.startsWith("!");

  // Only respond to DMs or @mentions
  if (!isDM && !isMentioned) return;

  // Strip the mention from the message
  let content = msg.content
    .replace(new RegExp(`<@!?${client.user!.id}>`, "g"), "")
    .replace(/^!\s*/, "")
    .trim();

  if (!content) {
    await msg.reply(
      "What's up? Ask me anything or use a slash command like `/status`."
    );
    return;
  }

  // Show typing indicator
  const typing = msg.channel.sendTyping();

  // Prefix with user context
  const userContext = isDM ? `[DM from ${msg.author.username}]` : "";
  const fullMessage = userContext ? `${userContext} ${content}` : content;

  const response = await chatWithHomeGPT(fullMessage, msg.author.id);
  const chunks = splitMessage(response);

  for (const chunk of chunks) {
    await msg.reply(chunk);
  }
}

// --- Slash Commands ---

const commands = [
  new SlashCommandBuilder()
    .setName("status")
    .setDescription("Get HomeGPT system status and pending alerts"),
  new SlashCommandBuilder()
    .setName("calendar")
    .setDescription("Show today's calendar events"),
  new SlashCommandBuilder()
    .setName("shopping")
    .setDescription("Show or manage the shopping list"),
  new SlashCommandBuilder()
    .setName("ask")
    .setDescription("Ask HomeGPT anything")
    .addStringOption((opt) =>
      opt
        .setName("question")
        .setDescription("Your question")
        .setRequired(true)
    ),
  new SlashCommandBuilder()
    .setName("briefing")
    .setDescription("Get the morning briefing summary"),
];

async function registerCommands() {
  if (!GUILD_ID) {
    console.warn("No DISCORD_GUILD_ID set, skipping slash command registration");
    return;
  }

  const rest = new REST().setToken(BOT_TOKEN);
  try {
    await rest.put(
      Routes.applicationGuildCommands(client.user!.id, GUILD_ID),
      { body: commands.map((c) => c.toJSON()) }
    );
    console.log("Slash commands registered");
  } catch (e) {
    console.error("Failed to register commands:", e);
  }
}

async function handleInteraction(interaction: Interaction) {
  if (!interaction.isChatInputCommand()) return;

  await interaction.deferReply();

  let question: string;

  switch (interaction.commandName) {
    case "status":
      question =
        "Give me a quick status update. Check the ergotools status, any pending alerts, and general system health.";
      break;
    case "calendar":
      question = "What's on the calendar today? List all events.";
      break;
    case "shopping":
      question =
        "What's on the shopping list? Read memory/food/shopping-lists.md";
      break;
    case "ask":
      question = interaction.options.getString("question") || "Hello";
      break;
    case "briefing":
      question =
        "Give me the morning briefing: today's calendar, any business alerts, school schedule, and anything else important.";
      break;
    default:
      await interaction.editReply("Unknown command");
      return;
  }

  const response = await chatWithHomeGPT(
    `[Discord slash command: /${interaction.commandName}] ${question}`,
    interaction.user.id
  );
  const chunks = splitMessage(response);

  await interaction.editReply(chunks[0]);
  for (const chunk of chunks.slice(1)) {
    await interaction.followUp(chunk);
  }
}

// --- Proactive Messaging (for heartbeat integration) ---

async function sendToChannel(
  channelName: string,
  content: string,
  embed?: Record<string, unknown>
): Promise<boolean> {
  const channelId =
    CHANNEL_MAP[channelName] ||
    process.env[`DISCORD_CHANNEL_${channelName.toUpperCase()}`];

  if (!channelId) {
    console.error(`No channel mapped for: ${channelName}`);
    return false;
  }

  try {
    const channel = await client.channels.fetch(channelId);
    if (!channel || !(channel instanceof TextChannel)) {
      console.error(`Channel ${channelName} (${channelId}) is not a text channel`);
      return false;
    }

    if (embed) {
      const embedObj = new EmbedBuilder()
        .setTitle((embed.title as string) || "HomeGPT")
        .setDescription((embed.description as string) || content)
        .setColor((embed.color as number) || 0x4fc3f7)
        .setTimestamp();

      if (embed.fields && Array.isArray(embed.fields)) {
        for (const field of embed.fields) {
          embedObj.addFields({
            name: field.name || "Info",
            value: field.value || "",
            inline: field.inline || false,
          });
        }
      }

      await channel.send({ embeds: [embedObj] });
    } else {
      const chunks = splitMessage(content);
      for (const chunk of chunks) {
        await channel.send(chunk);
      }
    }

    return true;
  } catch (e) {
    console.error(`Failed to send to ${channelName}:`, e);
    return false;
  }
}

async function sendDM(
  userId: string,
  content: string,
  embed?: Record<string, unknown>
): Promise<boolean> {
  try {
    const user = await client.users.fetch(userId);
    if (!user) return false;

    if (embed) {
      const embedObj = new EmbedBuilder()
        .setTitle((embed.title as string) || "HomeGPT")
        .setDescription((embed.description as string) || content)
        .setColor((embed.color as number) || 0x4fc3f7)
        .setTimestamp();

      await user.send({ embeds: [embedObj] });
    } else {
      const chunks = splitMessage(content);
      for (const chunk of chunks) {
        await user.send(chunk);
      }
    }

    return true;
  } catch (e) {
    console.error(`Failed to DM ${userId}:`, e);
    return false;
  }
}

// --- Internal HTTP API (for heartbeat to call) ---

function parseBody(req: IncomingMessage): Promise<Record<string, unknown>> {
  return new Promise((resolve, reject) => {
    let body = "";
    req.on("data", (chunk: Buffer) => (body += chunk.toString()));
    req.on("end", () => {
      try {
        resolve(body ? JSON.parse(body) : {});
      } catch {
        reject(new Error("Invalid JSON"));
      }
    });
  });
}

function sendJson(res: ServerResponse, data: unknown, status = 200) {
  res.writeHead(status, { "Content-Type": "application/json" });
  res.end(JSON.stringify(data));
}

const internalServer = createServer(async (req, res) => {
  const url = new URL(req.url || "/", `http://localhost:${BOT_PORT}`);

  // Health check
  if (url.pathname === "/health") {
    sendJson(res, {
      status: client.isReady() ? "ok" : "connecting",
      guilds: client.guilds.cache.size,
      channels: Object.keys(CHANNEL_MAP),
    });
    return;
  }

  // Send to channel: POST /send/channel
  if (url.pathname === "/send/channel" && req.method === "POST") {
    try {
      const body = await parseBody(req);
      const ok = await sendToChannel(
        body.channel as string,
        body.content as string,
        body.embed as Record<string, unknown> | undefined
      );
      sendJson(res, { sent: ok });
    } catch (e: any) {
      sendJson(res, { error: e.message }, 500);
    }
    return;
  }

  // Send DM: POST /send/dm
  if (url.pathname === "/send/dm" && req.method === "POST") {
    try {
      const body = await parseBody(req);
      const ok = await sendDM(
        body.userId as string,
        body.content as string,
        body.embed as Record<string, unknown> | undefined
      );
      sendJson(res, { sent: ok });
    } catch (e: any) {
      sendJson(res, { error: e.message }, 500);
    }
    return;
  }

  // Broadcast to all owners: POST /send/owners
  if (url.pathname === "/send/owners" && req.method === "POST") {
    try {
      const body = await parseBody(req);
      const results: Record<string, boolean> = {};
      for (const ownerId of OWNER_IDS) {
        results[ownerId] = await sendDM(
          ownerId,
          body.content as string,
          body.embed as Record<string, unknown> | undefined
        );
      }
      sendJson(res, { sent: results });
    } catch (e: any) {
      sendJson(res, { error: e.message }, 500);
    }
    return;
  }

  res.writeHead(404);
  res.end("Not found");
});

// --- Startup ---

client.once("ready", async () => {
  console.log(`Discord bot logged in as ${client.user!.tag}`);

  // Auto-detect channels by name
  if (GUILD_ID) {
    try {
      const guild = await client.guilds.fetch(GUILD_ID);
      const channels = await guild.channels.fetch();

      for (const name of CHANNEL_NAMES) {
        const envKey = `DISCORD_CHANNEL_${name.toUpperCase()}`;
        if (process.env[envKey]) {
          CHANNEL_MAP[name] = process.env[envKey]!;
        } else {
          const found = channels.find(
            (ch) => ch?.name === name || ch?.name === `homegpt-${name}`
          );
          if (found) {
            CHANNEL_MAP[name] = found.id;
          }
        }
      }

      console.log("Channel mapping:", CHANNEL_MAP);
    } catch (e) {
      console.error("Failed to auto-detect channels:", e);
    }
  }

  await registerCommands();

  // Start internal HTTP server
  internalServer.listen(BOT_PORT, "127.0.0.1", () => {
    console.log(`Internal API listening on http://127.0.0.1:${BOT_PORT}`);
    console.log("Endpoints:");
    console.log("  GET  /health          - Bot status");
    console.log("  POST /send/channel    - Send to channel {channel, content, embed?}");
    console.log("  POST /send/dm         - Send DM {userId, content, embed?}");
    console.log("  POST /send/owners     - Broadcast to owners {content, embed?}");
  });

  console.log(`HomeGPT URL: ${HOMEGPT_URL}`);
  console.log(`Owner IDs: ${OWNER_IDS.join(", ") || "(none set)"}`);
});

client.on("messageCreate", handleMessage);
client.on("interactionCreate", handleInteraction);

client.login(BOT_TOKEN);
