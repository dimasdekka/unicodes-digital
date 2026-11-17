import { listActiveProducts } from "@/lib/catalog";
import { BuyButton } from "./buy-button";

const rupiah = (n: number) => `Rp${n.toLocaleString("id-ID")}`;

// Halaman pembeli = katalog saja. Pembeli tidak perlu tahu soal supplier,
// saldo grosir, atau maintenance — info internal itu hanya ada di /admin.
// Produk yang belum bisa dijual tampil sebagai "Segera hadir" (tanpa alasan).
export default function Home() {
  const products = listActiveProducts();

  return (
    <>
      <style>{`
        .product-card { transition: transform 0.15s ease, box-shadow 0.15s ease; }
        .product-card:hover { transform: translateY(-4px); box-shadow: 0 12px 32px rgba(124, 58, 237, 0.35); }
      `}</style>
      <h1
        style={{
          color: "#ede9fe",
          fontSize: 32,
          margin: "8px 0 4px",
          background:
            "linear-gradient(90deg, #ede9fe 30%, #e8c874 70%)",
          WebkitBackgroundClip: "text",
          backgroundClip: "text",
          WebkitTextFillColor: "transparent",
        }}
      >
        Produk Digital
      </h1>
      <p style={{ color: "#b3a1e6", marginTop: 0, marginBottom: 24 }}>
        Produk digital instan — bayar lewat QRIS/e-wallet/VA, order langsung
        diproses dan hasilnya dikirim ke kamu.
      </p>

      <ul
        style={{
          listStyle: "none",
          padding: 0,
          display: "grid",
          gridTemplateColumns: "repeat(auto-fit, minmax(280px, 1fr))",
          gap: 20,
        }}
      >
        {products.map((p) => {
          const accent = p.accent ?? "#7c3aed";
          const isToken = p.id.startsWith("muse-");
          return (
            <li
              key={p.id}
              className="product-card"
              style={{
                border: "1px solid #7c3aed",
                borderRadius: 16,
                background: "#221244",
                overflow: "hidden",
                display: "flex",
                flexDirection: "column",
                opacity: p.comingSoon ? 0.85 : 1,
              }}
            >
              {/* Visual atas: gradient aksen + ikon produk */}
              <div
                style={{
                  height: 110,
                  background: `radial-gradient(circle at 30% 30%, ${accent}cc, #14092b 75%)`,
                  display: "flex",
                  alignItems: "center",
                  justifyContent: "center",
                  position: "relative",
                }}
              >
                {isToken ? (
                  <svg width="64" height="64" viewBox="0 0 64 64" aria-hidden>
                    <circle
                      cx="32"
                      cy="32"
                      r="26"
                      fill="#e8c874"
                      stroke="#fff7d6"
                      strokeWidth="2"
                    />
                    <circle
                      cx="32"
                      cy="32"
                      r="19"
                      fill="none"
                      stroke="#8a6d1f"
                      strokeWidth="1.5"
                      strokeDasharray="3 3"
                    />
                    <text
                      x="32"
                      y="41"
                      textAnchor="middle"
                      fontSize="26"
                      fontWeight="700"
                      fill="#3d2a00"
                    >
                      M
                    </text>
                  </svg>
                ) : (
                  <svg width="64" height="64" viewBox="0 0 64 64" aria-hidden>
                    <path
                      d="M32 6 L38 26 L58 32 L38 38 L32 58 L26 38 L6 32 L26 26 Z"
                      fill="#e8c874"
                      stroke="#fff7d6"
                      strokeWidth="1.5"
                    />
                    <path
                      d="M50 8 L52.5 16 L60 18.5 L52.5 21 L50 29 L47.5 21 L40 18.5 L47.5 16 Z"
                      fill="#ede9fe"
                    />
                  </svg>
                )}
                {p.badge && (
                  <span
                    style={{
                      position: "absolute",
                      top: 10,
                      right: 10,
                      background: "#e8c874",
                      color: "#3d2a00",
                      fontSize: 12,
                      fontWeight: 700,
                      borderRadius: 999,
                      padding: "3px 10px",
                    }}
                  >
                    {p.badge}
                  </span>
                )}
                {p.comingSoon && (
                  <span
                    style={{
                      position: "absolute",
                      top: 10,
                      left: 10,
                      background: "#14092b",
                      color: "#e8c874",
                      fontSize: 12,
                      fontWeight: 700,
                      border: "1px solid #d9b45b",
                      borderRadius: 999,
                      padding: "3px 10px",
                    }}
                  >
                    Segera hadir
                  </span>
                )}
              </div>

              <div
                style={{
                  padding: 16,
                  display: "flex",
                  flexDirection: "column",
                  gap: 8,
                  flex: 1,
                }}
              >
                <strong style={{ color: "#ede9fe", fontSize: 18 }}>
                  {p.name}
                </strong>
                <p style={{ margin: 0, color: "#c9b8f5", fontSize: 14 }}>
                  {p.description}
                </p>
                {p.features && (
                  <ul
                    style={{
                      listStyle: "none",
                      padding: 0,
                      margin: "4px 0",
                      display: "grid",
                      gap: 6,
                    }}
                  >
                    {p.features.map((f) => (
                      <li
                        key={f}
                        style={{ color: "#b3a1e6", fontSize: 13 }}
                      >
                        <span style={{ color: "#e8c874" }}>✓</span> {f}
                      </li>
                    ))}
                  </ul>
                )}
                <p style={{ margin: "8px 0 0" }}>
                  <strong
                    style={{ color: "#e8c874", fontSize: 24 }}
                  >
                    {rupiah(p.retailPriceIdr)}
                  </strong>{" "}
                  {p.unit && (
                    <span style={{ color: "#b3a1e6", fontSize: 13 }}>
                      {p.unit}
                    </span>
                  )}
                </p>
                <div style={{ marginTop: "auto", paddingTop: 8 }}>
                  <BuyButton
                    productId={p.id}
                    requiresTarget={p.requiresTarget}
                    targetLabel={p.targetLabel}
                    disabled={p.comingSoon}
                  />
                </div>
              </div>
            </li>
          );
        })}
      </ul>

      {/* Kepercayaan pembeli — singkat, tanpa urusan dapur */}
      <div
        style={{
          marginTop: 28,
          display: "flex",
          flexWrap: "wrap",
          gap: 12,
          justifyContent: "center",
        }}
      >
        {[
          "Proses instan setelah bayar",
          "Pembayaran aman — QRIS / e-wallet / VA",
          "Gagal diproses? Uang kembali",
        ].map((t) => (
          <span
            key={t}
            style={{
              color: "#b3a1e6",
              fontSize: 13,
              border: "1px solid #6d28d9",
              borderRadius: 999,
              padding: "6px 14px",
            }}
          >
            <span style={{ color: "#e8c874" }}>✓</span> {t}
          </span>
        ))}
      </div>
    </>
  );
}
