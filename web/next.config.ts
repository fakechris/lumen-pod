import type { NextConfig } from "next";

// API_INTERNAL_URL for Docker (service name), NEXT_PUBLIC_API_URL for local dev
const apiTarget = process.env.API_INTERNAL_URL ?? process.env.NEXT_PUBLIC_API_URL ?? "http://localhost:3210";

const nextConfig: NextConfig = {
  output: "standalone",
  turbopack: {
    rules: {
      "*.css": {
        loaders: ["@tailwindcss/turbopack"],
        as: "*.css",
      },
    },
  },
  async rewrites() {
    return [
      { source: "/api/:path*", destination: `${apiTarget}/api/:path*` },
      { source: "/auth/:path*", destination: `${apiTarget}/auth/:path*` },
      { source: "/audio/:path*", destination: `${apiTarget}/audio/:path*` },
      { source: "/static/:path*", destination: `${apiTarget}/static/:path*` },
      { source: "/healthz", destination: `${apiTarget}/healthz` },
    ];
  },
};

export default nextConfig;
