import { NextResponse } from "next/server";

export async function GET() {
  return NextResponse.json({
    message: "Hello from Next.js on Azure Functions!",
    framework: "Next.js",
    adapter: "Azure Functions Web Adapter",
    timestamp: new Date().toISOString(),
  });
}
