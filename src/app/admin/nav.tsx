// Navigasi antar halaman admin (dipakai semua halaman /admin*).
export default function AdminNav({ active }: { active: string }) {
  const link = (href: string, label: string) => (
    <a
      key={href}
      href={href}
      style={{
        color: active === href ? "#fff" : "#b3a1e6",
        fontWeight: active === href ? 700 : 400,
        marginRight: 16,
        fontSize: 14,
      }}
    >
      {label}
    </a>
  );
  return (
    <nav
      style={{
        marginBottom: 20,
        paddingBottom: 12,
        borderBottom: "1px solid #6d28d9",
      }}
    >
      {link("/admin", "🏠 Dashboard")}
      {link("/admin/tickets", "🎫 Tiket Support")}
      <a
        href="/api/admin/logout"
        style={{ color: "#b3a1e6", fontSize: 13, float: "right" }}
      >
        Keluar
      </a>
    </nav>
  );
}
