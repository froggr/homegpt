#!/usr/bin/env -S npx tsx
/**
 * ErgoTools Heartbeat Monitor
 *
 * Queries the ErgoTools PocketBase instance via its MCP tools and writes
 * a status summary to the HomeGPT workspace. Can be run as a cron job
 * or called by HomeGPT's heartbeat system.
 *
 * Usage:
 *   npx tsx scripts/ergotools-heartbeat.ts
 *
 * Environment:
 *   POCKETBASE_URL    - PocketBase API URL (default: https://app.ergonomicshelp.com)
 *   HOMEGPT_WORKSPACE - HomeGPT workspace path (default: ~/.homegpt/workspace)
 *   DISCORD_WEBHOOK   - Discord webhook URL for alerts (optional)
 */

import { writeFileSync, existsSync, mkdirSync, appendFileSync } from "fs";
import { join } from "path";
import { homedir } from "os";
import { execSync } from "child_process";

const PB_URL = process.env.POCKETBASE_URL || "https://app.ergonomicshelp.com";
const WORKSPACE =
  process.env.HOMEGPT_WORKSPACE || join(homedir(), ".homegpt", "workspace");
const DISCORD_BOT_URL = process.env.DISCORD_BOT_URL || "http://127.0.0.1:31342";
const STATUS_FILE = join(WORKSPACE, "memory", "business", "ergotools-status.md");

interface PBListResult<T> {
  page: number;
  perPage: number;
  totalItems: number;
  totalPages: number;
  items: T[];
}

async function pbFetch<T>(
  collection: string,
  filter?: string,
  sort?: string
): Promise<PBListResult<T>> {
  const params = new URLSearchParams({ perPage: "50" });
  if (filter) params.set("filter", filter);
  if (sort) params.set("sort", sort);

  const url = `${PB_URL}/api/collections/${collection}/records?${params}`;
  const res = await fetch(url);
  if (!res.ok) {
    throw new Error(`PocketBase ${collection}: ${res.status} ${res.statusText}`);
  }
  return res.json();
}

interface StatusSummary {
  pendingReviews: number;
  flaggedReviews: number;
  pendingProducts: number;
  staleAnnouncements: number;
  upcomingEvents: number;
  newSubmissions: number;
  alerts: string[];
}

async function checkStatus(): Promise<StatusSummary> {
  const summary: StatusSummary = {
    pendingReviews: 0,
    flaggedReviews: 0,
    pendingProducts: 0,
    staleAnnouncements: 0,
    upcomingEvents: 0,
    newSubmissions: 0,
    alerts: [],
  };

  try {
    // Pending product moderation
    const products = await pbFetch("products", "status = 'pending'");
    summary.pendingProducts = products.totalItems;
    if (summary.pendingProducts > 0) {
      summary.alerts.push(
        `${summary.pendingProducts} products pending moderation`
      );
    }
  } catch (e) {
    console.error("Failed to check products:", e);
  }

  try {
    // Pending/flagged reviews
    const pendingReviews = await pbFetch(
      "reviews",
      "status = 'pending' || status = 'flagged'"
    );
    for (const review of pendingReviews.items as any[]) {
      if (review.status === "pending") summary.pendingReviews++;
      if (review.status === "flagged") summary.flaggedReviews++;
    }
    if (summary.pendingReviews > 5) {
      summary.alerts.push(
        `${summary.pendingReviews} reviews pending (> 5 threshold)`
      );
    }
    if (summary.flaggedReviews > 0) {
      summary.alerts.push(`${summary.flaggedReviews} flagged reviews need attention`);
    }
  } catch (e) {
    console.error("Failed to check reviews:", e);
  }

  try {
    // Stale announcements (expired)
    const now = new Date().toISOString().slice(0, 10);
    const announcements = await pbFetch(
      "announcements",
      `end_date < '${now}' && status = 'active'`
    );
    summary.staleAnnouncements = announcements.totalItems;
    if (summary.staleAnnouncements > 0) {
      summary.alerts.push(
        `${summary.staleAnnouncements} expired announcements still active`
      );
    }
  } catch (e) {
    console.error("Failed to check announcements:", e);
  }

  try {
    // Upcoming events
    const now = new Date().toISOString();
    const events = await pbFetch("events", `date >= '${now}'`, "date");
    summary.upcomingEvents = events.totalItems;
  } catch (e) {
    console.error("Failed to check events:", e);
  }

  try {
    // New product submissions
    const submissions = await pbFetch(
      "product_submissions",
      "status = 'pending'"
    );
    summary.newSubmissions = submissions.totalItems;
    if (summary.newSubmissions > 0) {
      summary.alerts.push(
        `${summary.newSubmissions} new product submissions`
      );
    }
  } catch (e) {
    console.error("Failed to check submissions:", e);
  }

  return summary;
}

function writeStatusFile(summary: StatusSummary): void {
  const dir = join(WORKSPACE, "memory", "business");
  if (!existsSync(dir)) {
    mkdirSync(dir, { recursive: true });
  }

  const now = new Date().toISOString();
  const content = `---
category: business
last_verified: "${now}"
sources: [heartbeat]
---
# ErgoTools Business Status

Last updated: ${now}

## Pending Reviews
${summary.pendingReviews > 0 ? `${summary.pendingReviews} pending` : "None"}

## Flagged Content
${summary.flaggedReviews > 0 ? `**${summary.flaggedReviews} flagged reviews need attention**` : "None"}

## Pending Products
${summary.pendingProducts > 0 ? `${summary.pendingProducts} products awaiting moderation` : "None"}

## Stale Announcements
${summary.staleAnnouncements > 0 ? `${summary.staleAnnouncements} expired but still active` : "None"}

## Upcoming Events
${summary.upcomingEvents > 0 ? `${summary.upcomingEvents} upcoming` : "None scheduled"}

## Product Submissions
${summary.newSubmissions > 0 ? `${summary.newSubmissions} new submissions` : "None"}
`;

  writeFileSync(STATUS_FILE, content);
  console.log(`Status written to ${STATUS_FILE}`);

  // Append to daily log if there are alerts
  if (summary.alerts.length > 0) {
    const today = new Date().toISOString().slice(0, 10);
    const logFile = join(WORKSPACE, "memory", `${today}.md`);
    const logEntry = `\n## ErgoTools Alert (${new Date().toLocaleTimeString()})\n${summary.alerts.map((a) => `- ${a}`).join("\n")}\n`;
    appendFileSync(logFile, logEntry);
    console.log(`Alert logged to ${logFile}`);
  }
}

async function sendDiscordAlert(summary: StatusSummary): Promise<void> {
  if (summary.alerts.length === 0) return;

  const embed = {
    title: "ErgoTools Alert",
    color: 0xff6600,
    description: summary.alerts.map((a) => `- ${a}`).join("\n"),
    fields: [
      { name: "Pending Reviews", value: String(summary.pendingReviews), inline: true },
      { name: "Flagged", value: String(summary.flaggedReviews), inline: true },
      { name: "Pending Products", value: String(summary.pendingProducts), inline: true },
    ],
  };

  try {
    // Send to #ergotools channel via Discord bot
    await fetch(`${DISCORD_BOT_URL}/send/channel`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ channel: "ergotools", content: "", embed }),
    });
    console.log("Discord alert sent via bot");
  } catch (e) {
    console.error("Failed to send Discord alert (is discord-bot running?):", e);
  }

  // Also DM the owners for urgent alerts (flagged content)
  if (summary.flaggedReviews > 0) {
    try {
      await fetch(`${DISCORD_BOT_URL}/send/owners`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
          content: `**ErgoTools needs attention:** ${summary.flaggedReviews} flagged review(s) require moderation.`,
        }),
      });
    } catch {
      // Best effort
    }
  }
}

function sendDesktopNotification(summary: StatusSummary): void {
  if (summary.alerts.length === 0) return;

  const message = summary.alerts.join("; ");
  try {
    // macOS notification
    if (process.platform === "darwin") {
      execSync(
        `osascript -e 'display notification "${message}" with title "ErgoTools"'`
      );
    }
    // Linux notification
    if (process.platform === "linux") {
      execSync(`notify-send "ErgoTools" "${message}" 2>/dev/null || true`);
    }
  } catch {
    // Notification is best-effort
  }
}

async function main() {
  console.log(`ErgoTools heartbeat check: ${new Date().toISOString()}`);
  console.log(`PocketBase URL: ${PB_URL}`);

  const summary = await checkStatus();

  writeStatusFile(summary);

  if (summary.alerts.length > 0) {
    console.log("Alerts:", summary.alerts);
    await sendDiscordAlert(summary);
    sendDesktopNotification(summary);
  } else {
    console.log("All clear - no alerts");
  }
}

main().catch((e) => {
  console.error("Heartbeat failed:", e);
  process.exit(1);
});
