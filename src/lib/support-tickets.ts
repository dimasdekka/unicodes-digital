// Tiket support — pengganti forward pesan langsung ke Telegram admin
// (forward langsung DIMATIKAN permanen 2026-11-21; Dim tidak mau nomor
// pribadinya tampil ke user). Alur:
//   - User buka tiket dari bot (tombol 💬 Support -> kirim satu pesan).
//   - Pesan user tersimpan di support_messages; admin menjawab dari web
//     admin (/admin/tickets) dan balasan diteruskan ke user via Bot API.
//   - Satu user bisa punya banyak tiket, tapi hanya SATU yang terbuka
//     (open/pending) — pesan baru masuk ke tiket terbuka itu; kalau tiket
//     sudah ditutup, pesan baru membuka tiket baru.
//   - Bot berjalan sebagai proses terpisah, jadi balasan admin dikirim
//     lewat fetch langsung ke api.telegram.org (bukan lewat instance bot).
import { randomInt } from "node:crypto";

import { telegramBotToken } from "./config";
import { getDb } from "./db";

export type TicketStatus = "open" | "pending" | "closed";

export interface SupportTicket {
  id: string;
  tgId: number;
  username: string | null;
  firstName: string | null;
  status: TicketStatus;
  subject: string;
  createdAt: string;
  updatedAt: string;
  lastMessageAt: string;
}

export interface SupportMessage {
  id: number;
  ticketId: string;
  sender: "user" | "admin";
  text: string;
  createdAt: string;
}

/** Buat tabel tiket kalau belum ada (lazy — dipanggil tiap fungsi publik). */
export function ensureSupportTables(): void {
  getDb().exec(`
    CREATE TABLE IF NOT EXISTS support_tickets (
      id TEXT PRIMARY KEY,
      tg_id INTEGER NOT NULL,
      username TEXT,
      first_name TEXT,
      status TEXT NOT NULL DEFAULT 'open',
      subject TEXT NOT NULL DEFAULT '',
      created_at TEXT NOT NULL,
      updated_at TEXT NOT NULL,
      last_message_at TEXT NOT NULL
    );
    CREATE INDEX IF NOT EXISTS idx_support_tickets_status ON support_tickets(status);
    CREATE INDEX IF NOT EXISTS idx_support_tickets_user ON support_tickets(tg_id);
    CREATE TABLE IF NOT EXISTS support_messages (
      id INTEGER PRIMARY KEY AUTOINCREMENT,
      ticket_id TEXT NOT NULL,
      sender TEXT NOT NULL,
      text TEXT NOT NULL,
      created_at TEXT NOT NULL
    );
    CREATE INDEX IF NOT EXISTS idx_support_messages_ticket ON support_messages(ticket_id);
  `);
}

const TICKET_ALPHABET = "ABCDEFGHJKLMNPQRSTUVWXYZ23456789"; // tanpa 0/O/1/I

function newTicketId(): string {
  let suffix = "";
  for (let i = 0; i < 6; i++) {
    suffix += TICKET_ALPHABET[randomInt(TICKET_ALPHABET.length)];
  }
  return `TKT-${suffix}`;
}

function rowToTicket(row: Record<string, unknown>): SupportTicket {
  return {
    id: String(row.id),
    tgId: Number(row.tg_id),
    username: row.username == null ? null : String(row.username),
    firstName: row.first_name == null ? null : String(row.first_name),
    status: String(row.status) as TicketStatus,
    subject: String(row.subject ?? ""),
    createdAt: String(row.created_at),
    updatedAt: String(row.updated_at),
    lastMessageAt: String(row.last_message_at),
  };
}

function rowToMessage(row: Record<string, unknown>): SupportMessage {
  return {
    id: Number(row.id),
    ticketId: String(row.ticket_id),
    sender: String(row.sender) as "user" | "admin",
    text: String(row.text),
    createdAt: String(row.created_at),
  };
}

/** Tiket terbuka (open/pending) terbaru milik user, kalau ada. */
export function getOpenTicketByUser(tgId: number): SupportTicket | null {
  ensureSupportTables();
  const row = getDb()
    .prepare(
      `SELECT * FROM support_tickets
        WHERE tg_id = ? AND status IN ('open', 'pending')
        ORDER BY last_message_at DESC LIMIT 1`,
    )
    .get(tgId) as Record<string, unknown> | undefined;
  return row ? rowToTicket(row) : null;
}

/** Buat tiket baru dari pesan pertama user. */
export function createTicket(input: {
  tgId: number;
  username?: string | null;
  firstName?: string | null;
  text: string;
}): SupportTicket {
  ensureSupportTables();
  const now = new Date().toISOString();
  const subject = input.text.trim().slice(0, 60);
  // ID acak 6 karakter — tabrakan praktis tidak mungkin, tapi coba lagi
  // kalau kebetulan bentrok dengan tiket yang sudah ada.
  for (let attempt = 0; attempt < 5; attempt++) {
    const id = newTicketId();
    try {
      getDb()
        .prepare(
          `INSERT INTO support_tickets
            (id, tg_id, username, first_name, status, subject, created_at, updated_at, last_message_at)
            VALUES (?, ?, ?, ?, 'open', ?, ?, ?, ?)`,
        )
        .run(
          id,
          input.tgId,
          input.username ?? null,
          input.firstName ?? null,
          subject,
          now,
          now,
          now,
        );
      getDb()
        .prepare(
          `INSERT INTO support_messages (ticket_id, sender, text, created_at)
            VALUES (?, 'user', ?, ?)`,
        )
        .run(id, input.text.trim(), now);
      return {
        id,
        tgId: input.tgId,
        username: input.username ?? null,
        firstName: input.firstName ?? null,
        status: "open",
        subject,
        createdAt: now,
        updatedAt: now,
        lastMessageAt: now,
      };
    } catch {
      // Kemungkinan bentrok ID — coba ID lain.
    }
  }
  throw new Error("Gagal membuat tiket support (ID bentrok berulang)");
}

/** Tambah pesan user ke tiket terbuka; status kembali 'open' (butuh admin). */
export function addUserMessage(ticketId: string, text: string): void {
  ensureSupportTables();
  const now = new Date().toISOString();
  getDb()
    .prepare(
      `INSERT INTO support_messages (ticket_id, sender, text, created_at)
        VALUES (?, 'user', ?, ?)`,
    )
    .run(ticketId, text.trim(), now);
  getDb()
    .prepare(
      `UPDATE support_tickets
        SET status = 'open', updated_at = ?, last_message_at = ?
        WHERE id = ?`,
    )
    .run(now, now, ticketId);
}

/** Daftar tiket — terbuka dulu, lalu terbaru. */
export function listTickets(limit = 50): SupportTicket[] {
  ensureSupportTables();
  const rows = getDb()
    .prepare(
      `SELECT * FROM support_tickets
        ORDER BY CASE status WHEN 'open' THEN 0 WHEN 'pending' THEN 1 ELSE 2 END,
          last_message_at DESC LIMIT ?`,
    )
    .all(limit) as Record<string, unknown>[];
  return rows.map(rowToTicket);
}

/** Tiket milik satu user (untuk layar "Tiket Saya" di bot). */
export function listTicketsByUser(tgId: number, limit = 10): SupportTicket[] {
  ensureSupportTables();
  const rows = getDb()
    .prepare(
      `SELECT * FROM support_tickets WHERE tg_id = ?
        ORDER BY last_message_at DESC LIMIT ?`,
    )
    .all(tgId, limit) as Record<string, unknown>[];
  return rows.map(rowToTicket);
}

export function getTicket(id: string): SupportTicket | null {
  ensureSupportTables();
  const row = getDb()
    .prepare(`SELECT * FROM support_tickets WHERE id = ?`)
    .get(id) as Record<string, unknown> | undefined;
  return row ? rowToTicket(row) : null;
}

export function getTicketMessages(ticketId: string): SupportMessage[] {
  ensureSupportTables();
  const rows = getDb()
    .prepare(
      `SELECT * FROM support_messages WHERE ticket_id = ? ORDER BY id ASC`,
    )
    .all(ticketId) as Record<string, unknown>[];
  return rows.map(rowToMessage);
}

/** Jumlah tiket yang masih butuh perhatian admin (open + pending). */
export function countOpenTickets(): number {
  ensureSupportTables();
  const row = getDb()
    .prepare(
      `SELECT COUNT(*) AS n FROM support_tickets WHERE status IN ('open', 'pending')`,
    )
    .get() as { n: number };
  return row.n;
}

export function setTicketStatus(id: string, status: TicketStatus): void {
  ensureSupportTables();
  getDb()
    .prepare(`UPDATE support_tickets SET status = ?, updated_at = ? WHERE id = ?`)
    .run(status, new Date().toISOString(), id);
}

function escapeHtml(text: string): string {
  return text
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;");
}

/** Kirim pesan ke user lewat Bot API Telegram langsung (bot jalan sebagai
 *  proses terpisah — web admin tidak punya instance bot). */
export async function sendTelegramMessage(
  chatId: number,
  html: string,
): Promise<{ ok: boolean; error?: string }> {
  let token: string;
  try {
    token = telegramBotToken();
  } catch {
    return { ok: false, error: "TELEGRAM_BOT_TOKEN belum diisi di env" };
  }
  try {
    const res = await fetch(`https://api.telegram.org/bot${token}/sendMessage`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ chat_id: chatId, text: html, parse_mode: "HTML" }),
      signal: AbortSignal.timeout(15000),
    });
    if (!res.ok) return { ok: false, error: `Telegram API ${res.status}` };
    return { ok: true };
  } catch (err) {
    return {
      ok: false,
      error: err instanceof Error ? err.message : String(err),
    };
  }
}

/** Balas tiket: kirim ke user via Telegram + simpan + status jadi 'pending'
 *  (sudah dibalas admin, menunggu user). Pesan tetap disimpan walau kirim
 *  ke Telegram gagal — admin bisa kirim ulang dari web. */
export async function replyToTicket(
  ticketId: string,
  text: string,
): Promise<{ ok: boolean; error?: string }> {
  const ticket = getTicket(ticketId);
  if (!ticket) return { ok: false, error: "Tiket tidak ditemukan" };
  const trimmed = text.trim();
  if (!trimmed) return { ok: false, error: "Balasan kosong" };
  const sent = await sendTelegramMessage(
    ticket.tgId,
    `💬 <b>Balasan admin (tiket ${ticket.id}):</b>\n\n${escapeHtml(trimmed)}`,
  );
  ensureSupportTables();
  const now = new Date().toISOString();
  getDb()
    .prepare(
      `INSERT INTO support_messages (ticket_id, sender, text, created_at)
        VALUES (?, 'admin', ?, ?)`,
    )
    .run(ticketId, trimmed, now);
  getDb()
    .prepare(
      `UPDATE support_tickets
        SET status = 'pending', updated_at = ?, last_message_at = ?
        WHERE id = ?`,
    )
    .run(now, now, ticketId);
  return sent.ok ? { ok: true } : { ok: false, error: sent.error };
}

/** Tutup tiket + kabari user di Telegram. */
export async function closeTicket(
  ticketId: string,
): Promise<{ ok: boolean; error?: string }> {
  const ticket = getTicket(ticketId);
  if (!ticket) return { ok: false, error: "Tiket tidak ditemukan" };
  setTicketStatus(ticketId, "closed");
  const sent = await sendTelegramMessage(
    ticket.tgId,
    `🔒 Tiket <b>${ticket.id}</b> ditutup oleh admin. Kalau masalahmu belum selesai, buka tiket baru lewat tombol 💬 Support.`,
  );
  return sent.ok ? { ok: true } : { ok: false, error: sent.error };
}

/** Buka lagi tiket yang sudah ditutup. */
export function reopenTicket(ticketId: string): { ok: boolean; error?: string } {
  const ticket = getTicket(ticketId);
  if (!ticket) return { ok: false, error: "Tiket tidak ditemukan" };
  setTicketStatus(ticketId, "open");
  return { ok: true };
}
