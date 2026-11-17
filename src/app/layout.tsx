import type { Metadata } from "next";
import type { ReactNode } from "react";

export const metadata: Metadata = {
  title: "Unicodes Digital — Reseller AI & Digital",
  description:
    "Beli Muse, Gemini, dan produk digital lainnya — instan, bayar sekali, langsung diproses.",
};

export default function RootLayout({ children }: { children: ReactNode }) {
  return (
    <html lang="id">
      <body
        style={{
          fontFamily: "system-ui, sans-serif",
          margin: 0,
          background: "#14092b",
          color: "#ede9fe",
          minHeight: "100vh",
        }}
      >
        <header
          style={{
            padding: "16px 24px",
            background: "linear-gradient(90deg, #2a1650, #1a0f38)",
            borderBottom: "2px solid #d9b45b",
            display: "flex",
            justifyContent: "space-between",
            alignItems: "center",
          }}
        >
          <strong style={{ color: "#ede9fe", letterSpacing: 0.5 }}>
            ◆ Unicodes Digital
          </strong>
          <span style={{ fontSize: 13, color: "#d9b45b" }}>
            Reseller AI &amp; Digital
          </span>
        </header>
        <main style={{ maxWidth: 800, margin: "0 auto", padding: 24 }}>
          {children}
        </main>
      </body>
    </html>
  );
}
