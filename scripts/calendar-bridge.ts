#!/usr/bin/env -S npx tsx
/**
 * Google Calendar Bridge Service
 *
 * Small HTTP service that wraps Google Calendar API for HomeGPT.
 * Runs on port 31340 and provides simple JSON endpoints.
 *
 * Reuses OAuth config from the homeschool project.
 *
 * Usage:
 *   npx tsx scripts/calendar-bridge.ts
 *
 * Environment:
 *   GOOGLE_CLIENT_ID     - OAuth client ID
 *   GOOGLE_CLIENT_SECRET - OAuth client secret
 *   GOOGLE_REDIRECT_URI  - OAuth redirect (default: http://localhost:31340/oauth/callback)
 *   GOOGLE_REFRESH_TOKEN - OAuth refresh token (from homeschool .env)
 *   CALENDAR_PORT        - Port to listen on (default: 31340)
 *   CALENDAR_IDS         - Comma-separated calendar IDs to monitor
 */

import { createServer, IncomingMessage, ServerResponse } from "http";

const PORT = parseInt(process.env.CALENDAR_PORT || "31340");
const CLIENT_ID = process.env.GOOGLE_CLIENT_ID || "";
const CLIENT_SECRET = process.env.GOOGLE_CLIENT_SECRET || "";
const REDIRECT_URI =
  process.env.GOOGLE_REDIRECT_URI ||
  `http://localhost:${PORT}/oauth/callback`;
let REFRESH_TOKEN = process.env.GOOGLE_REFRESH_TOKEN || "";
let ACCESS_TOKEN = "";
let TOKEN_EXPIRY = 0;

const CALENDAR_IDS = (process.env.CALENDAR_IDS || "primary")
  .split(",")
  .map((id) => id.trim());

async function refreshAccessToken(): Promise<string> {
  if (ACCESS_TOKEN && Date.now() < TOKEN_EXPIRY - 60000) {
    return ACCESS_TOKEN;
  }

  if (!REFRESH_TOKEN) {
    throw new Error("No refresh token. Visit /oauth/start to authenticate.");
  }

  const res = await fetch("https://oauth2.googleapis.com/token", {
    method: "POST",
    headers: { "Content-Type": "application/x-www-form-urlencoded" },
    body: new URLSearchParams({
      client_id: CLIENT_ID,
      client_secret: CLIENT_SECRET,
      refresh_token: REFRESH_TOKEN,
      grant_type: "refresh_token",
    }),
  });

  if (!res.ok) {
    const text = await res.text();
    throw new Error(`Token refresh failed: ${res.status} ${text}`);
  }

  const data = await res.json();
  ACCESS_TOKEN = data.access_token;
  TOKEN_EXPIRY = Date.now() + data.expires_in * 1000;
  console.log("Access token refreshed");
  return ACCESS_TOKEN;
}

async function calendarFetch(
  path: string,
  params?: Record<string, string>,
  method: string = "GET",
  body?: any
): Promise<any> {
  const token = await refreshAccessToken();
  const url = new URL(`https://www.googleapis.com/calendar/v3${path}`);
  if (params) {
    Object.entries(params).forEach(([k, v]) => url.searchParams.set(k, v));
  }

  const res = await fetch(url.toString(), {
    method,
    headers: {
      Authorization: `Bearer ${token}`,
      "Content-Type": "application/json",
    },
    body: body ? JSON.stringify(body) : undefined,
  });

  if (!res.ok) {
    const text = await res.text();
    throw new Error(`Calendar API ${path}: ${res.status} ${text}`);
  }

  return res.json();
}

function todayRange(): { timeMin: string; timeMax: string } {
  const now = new Date();
  const start = new Date(now.getFullYear(), now.getMonth(), now.getDate());
  const end = new Date(start);
  end.setDate(end.getDate() + 1);
  return {
    timeMin: start.toISOString(),
    timeMax: end.toISOString(),
  };
}

function weekRange(): { timeMin: string; timeMax: string } {
  const now = new Date();
  const dayOfWeek = now.getDay();
  const start = new Date(now);
  start.setDate(now.getDate() - dayOfWeek);
  start.setHours(0, 0, 0, 0);
  const end = new Date(start);
  end.setDate(start.getDate() + 7);
  return {
    timeMin: start.toISOString(),
    timeMax: end.toISOString(),
  };
}

interface CalendarEvent {
  id: string;
  summary: string;
  start: string;
  end: string;
  allDay: boolean;
  calendarId: string;
}

function parseEvent(event: any, calendarId: string): CalendarEvent {
  const allDay = !!event.start?.date;
  return {
    id: event.id,
    summary: event.summary || "(No title)",
    start: allDay ? event.start.date : event.start.dateTime,
    end: allDay ? event.end.date : event.end.dateTime,
    allDay,
    calendarId,
  };
}

async function getEvents(
  range: { timeMin: string; timeMax: string }
): Promise<CalendarEvent[]> {
  const allEvents: CalendarEvent[] = [];

  for (const calendarId of CALENDAR_IDS) {
    try {
      const data = await calendarFetch(
        `/calendars/${encodeURIComponent(calendarId)}/events`,
        {
          timeMin: range.timeMin,
          timeMax: range.timeMax,
          singleEvents: "true",
          orderBy: "startTime",
          maxResults: "50",
        }
      );
      for (const item of data.items || []) {
        allEvents.push(parseEvent(item, calendarId));
      }
    } catch (e) {
      console.error(`Failed to fetch calendar ${calendarId}:`, e);
    }
  }

  // Sort by start time
  allEvents.sort((a, b) => a.start.localeCompare(b.start));
  return allEvents;
}

function sendJson(res: ServerResponse, data: any, status = 200) {
  res.writeHead(status, { "Content-Type": "application/json" });
  res.end(JSON.stringify(data));
}

function sendError(res: ServerResponse, msg: string, status = 500) {
  sendJson(res, { error: msg }, status);
}

async function parseBody(req: IncomingMessage): Promise<any> {
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

const server = createServer(async (req, res) => {
  const url = new URL(req.url || "/", `http://localhost:${PORT}`);
  const path = url.pathname;

  try {
    // Health check
    if (path === "/health") {
      sendJson(res, { status: "ok", calendars: CALENDAR_IDS });
      return;
    }

    // OAuth start
    if (path === "/oauth/start") {
      const authUrl = `https://accounts.google.com/o/oauth2/v2/auth?${new URLSearchParams(
        {
          client_id: CLIENT_ID,
          redirect_uri: REDIRECT_URI,
          response_type: "code",
          scope: "https://www.googleapis.com/auth/calendar",
          access_type: "offline",
          prompt: "consent",
        }
      )}`;
      res.writeHead(302, { Location: authUrl });
      res.end();
      return;
    }

    // OAuth callback
    if (path === "/oauth/callback") {
      const code = url.searchParams.get("code");
      if (!code) {
        sendError(res, "Missing code parameter", 400);
        return;
      }

      const tokenRes = await fetch("https://oauth2.googleapis.com/token", {
        method: "POST",
        headers: { "Content-Type": "application/x-www-form-urlencoded" },
        body: new URLSearchParams({
          client_id: CLIENT_ID,
          client_secret: CLIENT_SECRET,
          redirect_uri: REDIRECT_URI,
          code,
          grant_type: "authorization_code",
        }),
      });

      const data = await tokenRes.json();
      if (data.refresh_token) {
        REFRESH_TOKEN = data.refresh_token;
        console.log("Got refresh token:", REFRESH_TOKEN);
      }
      ACCESS_TOKEN = data.access_token;
      TOKEN_EXPIRY = Date.now() + data.expires_in * 1000;

      sendJson(res, {
        message: "Authenticated! Set GOOGLE_REFRESH_TOKEN in your env.",
        refresh_token: data.refresh_token,
      });
      return;
    }

    // Today's events
    if (path === "/events/today") {
      const events = await getEvents(todayRange());
      sendJson(res, { events, count: events.length });
      return;
    }

    // This week's events
    if (path === "/events/week") {
      const events = await getEvents(weekRange());
      sendJson(res, { events, count: events.length });
      return;
    }

    // Create event
    if (path === "/events/create" && req.method === "POST") {
      const body = await parseBody(req);
      const calendarId = body.calendarId || "primary";
      const event = await calendarFetch(
        `/calendars/${encodeURIComponent(calendarId)}/events`,
        {},
        "POST",
        {
          summary: body.summary,
          start: body.allDay
            ? { date: body.start }
            : { dateTime: body.start },
          end: body.allDay ? { date: body.end } : { dateTime: body.end },
          description: body.description,
        }
      );
      sendJson(res, { created: parseEvent(event, calendarId) });
      return;
    }

    // Move event
    if (path === "/events/move" && req.method === "POST") {
      const body = await parseBody(req);
      const calendarId = body.calendarId || "primary";
      const event = await calendarFetch(
        `/calendars/${encodeURIComponent(calendarId)}/events/${body.eventId}`,
        {},
        "PATCH",
        {
          start: body.allDay
            ? { date: body.newStart }
            : { dateTime: body.newStart },
          end: body.allDay
            ? { date: body.newEnd }
            : { dateTime: body.newEnd },
        }
      );
      sendJson(res, { updated: parseEvent(event, calendarId) });
      return;
    }

    sendError(res, "Not found", 404);
  } catch (e: any) {
    console.error(`Error handling ${path}:`, e.message);
    sendError(res, e.message);
  }
});

server.listen(PORT, "0.0.0.0", () => {
  console.log(`Calendar bridge listening on http://0.0.0.0:${PORT}`);
  console.log(`Calendars: ${CALENDAR_IDS.join(", ")}`);
  if (!REFRESH_TOKEN) {
    console.log(
      `No refresh token. Visit http://localhost:${PORT}/oauth/start to authenticate.`
    );
  }
});
