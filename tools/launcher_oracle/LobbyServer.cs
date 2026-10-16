// SPDX-License-Identifier: GPL-3.0-or-later

// The lobby stand-in: recorded answers of Kunos' lobby served on 127.0.0.1, so the original's
// getserverlist / getserver (its lobby host moved here with --lobby) and the port's (rustyac-launcher
// --lobby) read the same bytes. Every request it gets goes to the log: request line and headers,
// in the order they came, so what the two sides send can be compared.
//
//   launcher_oracle --serve <recordings> --port <n> --log <file>
//
// The recordings: list.json (lobby.ashx/list, sent gzipped as the lobby sends it) and
// single_<ip>_<port>.json (lobby.ashx/single). Anything else answers 404.

using System;
using System.IO;
using System.IO.Compression;
using System.Net;
using System.Net.Sockets;
using System.Text;
using System.Threading;
using System.Web;

static class LobbyServer
{
    public static int Serve(string folder, int port, string logFile)
    {
        var listener = new TcpListener(IPAddress.Loopback, port);
        listener.Start();
        Console.WriteLine("serving " + folder + " on 127.0.0.1:" + port);
        var log = new object();
        while (true)
        {
            TcpClient client = listener.AcceptTcpClient();
            new Thread(() =>
            {
                try { Answer(client, folder, logFile, log); }
                catch (Exception e) { Console.Error.WriteLine("lobby: " + e.Message); }
                finally { client.Close(); }
            }).Start();
        }
    }

    static void Answer(TcpClient client, string folder, string logFile, object log)
    {
        NetworkStream net = client.GetStream();
        var head = new StringBuilder();
        while (!head.ToString().EndsWith("\r\n\r\n"))
        {
            int b = net.ReadByte();
            if (b < 0) return;
            head.Append((char)b);
        }
        lock (log) File.AppendAllText(logFile, head.ToString());
        string target = head.ToString().Split(' ')[1];
        int q = target.IndexOf('?');
        string path = q < 0 ? target : target.Substring(0, q);
        var query = HttpUtility.ParseQueryString(q < 0 ? "" : target.Substring(q));
        string file = null;
        bool gzip = false;
        if (path.EndsWith("/lobby.ashx/list"))
        {
            file = Path.Combine(folder, "list.json");
            gzip = true;
        }
        else if (path.EndsWith("/lobby.ashx/single"))
            file = Path.Combine(folder, "single_" + query["ip"] + "_" + query["port"] + ".json");
        byte[] body = file != null && File.Exists(file) ? File.ReadAllBytes(file) : null;
        string status = body != null ? "200 OK" : "404 Not Found";
        body = body ?? Encoding.ASCII.GetBytes("not recorded");
        if (gzip)
        {
            var packed = new MemoryStream();
            using (var z = new GZipStream(packed, CompressionMode.Compress, true)) z.Write(body, 0, body.Length);
            body = packed.ToArray();
        }
        string reply = "HTTP/1.1 " + status + "\r\nContent-Type: application/json; charset=utf-8\r\n"
            + (gzip ? "Content-Encoding: gzip\r\n" : "") + "Content-Length: " + body.Length + "\r\nConnection: close\r\n\r\n";
        byte[] r = Encoding.ASCII.GetBytes(reply);
        net.Write(r, 0, r.Length);
        net.Write(body, 0, body.Length);
    }
}
