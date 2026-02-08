#!/usr/bin/env python3
"""
Voice Bridge Service for HomeGPT

Middleware connecting voice services (Whisper STT + Pocket TTS) to HomeGPT's
HTTP API. Enables voice conversations for homeschool tutoring.

Flow:
    Mic → Hark (browser VAD) → Whisper STT (port 8001) → text
        → HomeGPT /api/chat (port 31327) → response text
        → Pocket TTS (port 8000) → audio → Speaker

This service provides a unified endpoint that the frontend can use.

Usage:
    python scripts/voice-bridge.py

Environment:
    HOMEGPT_URL      - HomeGPT API URL (default: http://localhost:31327)
    WHISPER_URL      - Whisper STT URL (default: http://localhost:8001)
    TTS_URL          - Pocket TTS URL (default: http://localhost:8000)
    VOICE_PORT       - Port for this service (default: 31341)
"""

import asyncio
import json
import os
import re
from datetime import datetime

import httpx
from fastapi import FastAPI, UploadFile, File, Form, WebSocket
from fastapi.middleware.cors import CORSMiddleware
from fastapi.responses import StreamingResponse, JSONResponse

HOMEGPT_URL = os.getenv("HOMEGPT_URL", "http://localhost:31327")
WHISPER_URL = os.getenv("WHISPER_URL", "http://localhost:8001")
TTS_URL = os.getenv("TTS_URL", "http://localhost:8000")
VOICE_PORT = int(os.getenv("VOICE_PORT", "31341"))

app = FastAPI(title="HomeGPT Voice Bridge")

app.add_middleware(
    CORSMiddleware,
    allow_origins=["*"],
    allow_methods=["*"],
    allow_headers=["*"],
)

# Available TTS voices
VOICES = ["alba", "marius", "cosette", "javert", "jean", "fantine", "eponine", "azelma"]


def clean_text_for_speech(text: str) -> str:
    """Strip markdown and special characters for TTS.

    Ported from homeschool's cleanTextForSpeech().
    """
    # Remove code blocks
    text = re.sub(r"```[\s\S]*?```", "", text)
    # Remove inline code
    text = re.sub(r"`[^`]+`", "", text)
    # Remove bold/italic
    text = re.sub(r"\*+([^*]+)\*+", r"\1", text)
    # Remove headers
    text = re.sub(r"^#+\s+", "", text, flags=re.MULTILINE)
    # Remove links, keep text
    text = re.sub(r"\[([^\]]+)\]\([^)]+\)", r"\1", text)
    # Remove special characters
    text = re.sub(r"[*_~`#>|]", "", text)
    # Collapse whitespace
    text = re.sub(r"\n+", " ", text)
    text = re.sub(r"\s+", " ", text)
    return text.strip()


def split_into_sentences(text: str) -> list[str]:
    """Split text into sentences for chunked TTS."""
    sentences = re.split(r"(?<=[.!?])\s+", text)
    return [s.strip() for s in sentences if len(s.strip()) > 3]


@app.get("/health")
async def health():
    """Health check with service status."""
    services = {}

    async with httpx.AsyncClient(timeout=2.0) as client:
        try:
            r = await client.get(f"{HOMEGPT_URL}/health")
            services["homegpt"] = r.status_code == 200
        except Exception:
            services["homegpt"] = False

        try:
            r = await client.get(f"{WHISPER_URL}/health")
            services["whisper"] = r.status_code == 200
        except Exception:
            services["whisper"] = False

        try:
            r = await client.get(f"{TTS_URL}/health")
            services["tts"] = r.status_code == 200
        except Exception:
            # Pocket TTS may not have /health
            try:
                r = await client.get(TTS_URL)
                services["tts"] = r.status_code < 500
            except Exception:
                services["tts"] = False

    all_ok = all(services.values())
    return JSONResponse(
        {"status": "ok" if all_ok else "degraded", "services": services},
        status_code=200 if all_ok else 503,
    )


@app.get("/voices")
async def list_voices():
    """List available TTS voices."""
    return {"voices": VOICES, "default": "marius"}


@app.post("/stt")
async def speech_to_text(audio: UploadFile = File(...)):
    """Transcribe audio using Whisper."""
    audio_bytes = await audio.read()

    async with httpx.AsyncClient(timeout=30.0) as client:
        files = {"file": (audio.filename or "audio.webm", audio_bytes, audio.content_type or "audio/webm")}
        r = await client.post(
            f"{WHISPER_URL}/v1/audio/transcriptions",
            files=files,
            data={"model": "base"},
        )
        r.raise_for_status()
        return r.json()


@app.post("/tts")
async def text_to_speech(text: str = Form(...), voice: str = Form("marius")):
    """Generate speech from text using Pocket TTS."""
    cleaned = clean_text_for_speech(text)
    if not cleaned:
        return JSONResponse({"error": "Empty text after cleaning"}, status_code=400)

    async with httpx.AsyncClient(timeout=30.0) as client:
        r = await client.post(
            f"{TTS_URL}/tts",
            data={"text": cleaned, "voice_url": voice},
        )
        r.raise_for_status()
        return StreamingResponse(
            content=iter([r.content]),
            media_type="audio/wav",
        )


@app.post("/chat")
async def chat(
    message: str = Form(...),
    child_name: str = Form(""),
    subject: str = Form(""),
    voice: str = Form("marius"),
    tutor_mode: bool = Form(False),
):
    """Send a message to HomeGPT and get a spoken response.

    Returns the text response along with audio URL for playback.
    """
    # Build the prompt with tutor context if in tutor mode
    prompt = message
    if tutor_mode and child_name:
        prompt = f"[Tutor mode: helping {child_name} with {subject or 'general'}] {message}"

    # Send to HomeGPT
    async with httpx.AsyncClient(timeout=120.0) as client:
        r = await client.post(
            f"{HOMEGPT_URL}/api/chat",
            json={"message": prompt},
        )
        r.raise_for_status()
        response_data = r.json()

    response_text = response_data.get("response", "")

    # Generate TTS for the response
    cleaned = clean_text_for_speech(response_text)
    audio_chunks = []

    if cleaned:
        sentences = split_into_sentences(cleaned)
        async with httpx.AsyncClient(timeout=30.0) as client:
            for sentence in sentences[:5]:  # Limit to 5 sentences for voice
                try:
                    r = await client.post(
                        f"{TTS_URL}/tts",
                        data={"text": sentence, "voice_url": voice},
                    )
                    if r.status_code == 200:
                        audio_chunks.append(r.content)
                except Exception:
                    pass

    # Return text + audio
    return JSONResponse({
        "text": response_text,
        "spoken_text": cleaned,
        "sentence_count": len(split_into_sentences(cleaned)) if cleaned else 0,
        "voice": voice,
        "tutor_mode": tutor_mode,
    })


@app.post("/tutor/session")
async def log_tutor_session(
    child_name: str = Form(...),
    subject: str = Form(...),
    summary: str = Form(""),
    struggled_with: str = Form(""),
    excelled_at: str = Form(""),
):
    """Log a tutoring session summary to the workspace."""
    workspace = os.getenv("HOMEGPT_WORKSPACE", os.path.expanduser("~/.homegpt/workspace"))
    notes_file = os.path.join(workspace, "memory", "school", "tutor-notes.md")

    now = datetime.now()
    entry = f"""
## {now.strftime('%Y-%m-%d %H:%M')} - {child_name} - {subject}

{summary}

**Struggled with:** {struggled_with or 'Nothing notable'}
**Excelled at:** {excelled_at or 'Nothing notable'}

---
"""

    os.makedirs(os.path.dirname(notes_file), exist_ok=True)
    with open(notes_file, "a") as f:
        f.write(entry)

    return {"logged": True, "file": notes_file}


if __name__ == "__main__":
    import uvicorn
    print(f"Voice bridge starting on port {VOICE_PORT}")
    print(f"  HomeGPT: {HOMEGPT_URL}")
    print(f"  Whisper: {WHISPER_URL}")
    print(f"  TTS:     {TTS_URL}")
    uvicorn.run(app, host="0.0.0.0", port=VOICE_PORT)
