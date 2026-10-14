import type { Metadata } from "next";
import "./globals.css";

export const metadata: Metadata = {
  title: "Treefime",
  description: "A flexible workspace for organizing work.",
};

export default function RootLayout({ children }: Readonly<{ children: React.ReactNode }>) {
  return (
    <html lang="en">
      <body>{children}</body>
    </html>
  );
}
