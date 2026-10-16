// SPDX-License-Identifier: GPL-3.0-or-later

// Launcher oracle: Assetto Corsa's launcher (AssettoCorsa.exe) loaded as an assembly, never run
// as a program. Its own ACRequestHandler answers the theme's ac:// requests the way
// ACSchemeHandler.DoProcess hands them over, with every folder of the player's
// (Documents\Assetto Corsa) moved to a fake one under re\scratch\task35. The answers go to a
// transcript the Rust port writes too; the files the handlers write stay in the fake folder.
// See docs/port/launcher.md.
//
//   launcher_oracle --docs <fake folder> --requests <file> --out <transcript>
//
// A request file has one line a request: `GET <url>`, `POST <url><TAB><body>`, or
// `FORMAT <name>` (AppIniFile.Format of cfg\<name>.ini, what "start" does before it starts the
// game), or `SETUP` (Startup.SetupUserDirectory, what the launcher does with the player's folder
// when it starts). Blank lines and lines starting with # are skipped.

using System;
using System.Collections;
using System.Collections.Generic;
using System.Collections.Specialized;
using System.IO;
using System.Linq;
using System.Reflection;
using System.Text;
using System.Web;

static class Program
{
    const BindingFlags Any = BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Static;

    // The handlers that only read or write the player's folder (and read the game's). The
    // others start programs, talk to Steam or servers, or play sounds; the oracle refuses them.
    static readonly HashSet<string> Allowed = new HashSet<string> {
        "listcars", "listtracks", "listfiles", "listinis", "listseries", "listseasons",
        "listspecialevents", "listshowrooms", "getsetting", "getsettings", "setsetting",
        "delsetting", "initdata", "setdata", "getdata", "deldata", "copyini", "getiniasjson",
        "savejsontoini", "savetofile", "getcfg", "getskinmeta", "carcount", "eventcount",
        "getresults", "loadjson", "savejson", "delfile", "getplayersetups", "getthememodules",
        "utility", "getscripts", "js", "initjslibs", "getvideopage", "getplayername", "formatfile",
        "gethash", "getcars", "generatecss", "getvideomodes",
        "uimessage", "loading", "resizewindow", "beforeLoadModules", "loadModules", "uiready", "cleanup",
    };

    // Every name in ACRequestHandler.Handlers: a name not in it runs nothing (the answer is
    // ACRequestData(false)), so the oracle lets it through.
    static readonly HashSet<string> KnownHosts = new HashSet<string> {
        "exit", "home", "listcars", "listtracks", "listseasons", "listfiles", "listspecialevents",
        "listshowrooms", "savecombo", "start", "runtest", "calibrate", "showroom", "getsetting",
        "getsettings", "setsetting", "delsetting", "initdata", "setdata", "getdata", "deldata",
        "stopmusic", "startmusic", "setvolume", "utility", "copyini", "generatecss", "playsound",
        "showcontrols", "getiniasjson", "savejsontoini", "savetofile", "requestcode", "checklicense",
        "requestlicense", "copytoclipboard", "openforum", "browseto", "sendbenchmark", "sendcrash",
        "print", "addtoleaderboard", "getvideomodes", "initjslibs", "getcfg", "js", "getskinmeta",
        "carcount", "eventcount", "getscripts", "addreplay", "listreplays", "delreplay", "updreplay",
        "storereplay", "getserver", "getserverlist", "getlanservers", "getservercached", "formatfile",
        "pingserver", "getguid", "getresults", "getplayername", "getid", "isowner", "loadjson",
        "savejson", "getachievements", "getthememodules", "getplayersetups", "getvideopage",
        "showsteamoverlay", "gethash", "listseries", "listinis", "getcars", "delfile", "uimessage",
        "loading", "resizewindow", "uiready", "cleanup",
    };

    static int Main(string[] args)
    {
        string docs = null, requests = null, outFile = null;
        for (int i = 0; i + 1 < args.Length; i += 2)
        {
            switch (args[i])
            {
                case "--docs": docs = Path.GetFullPath(args[i + 1]).TrimEnd('\\') + "\\"; break;
                case "--requests": requests = args[i + 1]; break;
                case "--out": outFile = args[i + 1]; break;
                default: Console.Error.WriteLine("unknown option " + args[i]); return 2;
            }
        }
        if (docs == null || requests == null || outFile == null)
        {
            Console.Error.WriteLine("usage: launcher_oracle --docs <fake folder> --requests <file> --out <transcript>");
            return 2;
        }
        string personal = Environment.GetFolderPath(Environment.SpecialFolder.Personal).TrimEnd('\\') + "\\";
        if (docs.StartsWith(personal, StringComparison.OrdinalIgnoreCase) || docs.IndexOf("\\re\\scratch\\task35\\", StringComparison.OrdinalIgnoreCase) < 0)
        {
            Console.Error.WriteLine("refused: the fake folder must be under re\\scratch\\task35 and not in Documents: " + docs);
            return 2;
        }
        string acRoot = Environment.GetEnvironmentVariable("AC_ROOT") ?? @"C:\Program Files (x86)\Steam\steamapps\common\assettocorsa";
        acRoot = acRoot.TrimEnd('\\') + "\\";
        string realDocs = personal + "Assetto Corsa\\";
        var before = Snapshot(realDocs);

        AppDomain.CurrentDomain.AssemblyResolve += (sender, e) =>
        {
            string name = new AssemblyName(e.Name).Name;
            foreach (string folder in new[] { acRoot + "launcher\\support\\", acRoot })
            {
                foreach (string ext in new[] { ".dll", ".exe" })
                {
                    if (File.Exists(folder + name + ext))
                        return Assembly.LoadFrom(folder + name + ext);
                }
            }
            return null;
        };
        Assembly launcher = Assembly.LoadFrom(acRoot + "AssettoCorsa.exe");
        Type appFolder = launcher.GetType("AC.Launcher.Utility.AppFolder", true);
        string userData = (string)appFolder.GetField("UserDataPath").GetValue(null);
        foreach (FieldInfo field in appFolder.GetFields(BindingFlags.Public | BindingFlags.Static))
        {
            if (field.FieldType != typeof(string)) continue;
            string value = (string)field.GetValue(null);
            if (value != null && value.StartsWith(userData, StringComparison.OrdinalIgnoreCase))
                field.SetValue(null, docs + value.Substring(userData.Length));
        }
        foreach (FieldInfo field in appFolder.GetFields(BindingFlags.Public | BindingFlags.Static))
        {
            string value = field.GetValue(null) as string;
            if (value != null && value.StartsWith(realDocs, StringComparison.OrdinalIgnoreCase))
            {
                Console.Error.WriteLine("refused: AppFolder." + field.Name + " still points at Documents");
                return 3;
            }
        }
        Type handlerType = launcher.GetType("AC.Launcher.ACRequestHandler", true);
        Type appIni = launcher.GetType("AC.Launcher.Utility.AppIniFile", true);
        MethodInfo message = handlerType.GetMethod("Message", BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Static, null, new[] { typeof(string), typeof(bool) }, null);

        using (var transcript = new FileStream(outFile, FileMode.Create))
        {
            foreach (string raw in File.ReadAllLines(requests, Encoding.UTF8))
            {
                string line = raw.TrimEnd('\r');
                if (line.Length == 0 || line[0] == '#') continue;
                int space = line.IndexOf(' ');
                string verb = line.Substring(0, space), rest = line.Substring(space + 1);
                if (verb == "FORMAT" || verb == "SETUP")
                {
                    string result = "done";
                    try
                    {
                        if (verb == "FORMAT")
                        {
                            string file = (string)appIni.GetMethod("Get", Any).Invoke(null, new object[] { rest });
                            appIni.GetMethod("Format", Any).Invoke(null, new object[] { file });
                        }
                        else
                        {
                            // Startup.SetupUserDirectory: it creates AppPath\cfg when it is missing,
                            // so the game's cfg folder has to be there already
                            if (!Directory.Exists(acRoot + "cfg") || !Directory.Exists(acRoot + "cfg\\templates"))
                            {
                                Console.Error.WriteLine("refused: the game's cfg folder is not there");
                                return 2;
                            }
                            launcher.GetType("AC.Launcher.Startup", true).GetMethod("SetupUserDirectory", Any).Invoke(null, null);
                        }
                    }
                    catch (TargetInvocationException wrapped)
                    {
                        result = wrapped.InnerException.GetType().Name + ": " + wrapped.InnerException.Message;
                    }
                    Write(transcript, line, "-", false, Encoding.UTF8.GetBytes(result));
                    continue;
                }
                string url = rest, body = null;
                if (verb == "POST")
                {
                    int tab = rest.IndexOf('\t');
                    url = rest.Substring(0, tab);
                    body = rest.Substring(tab + 1);
                }
                // ACSchemeHandler.DoProcess
                Uri uri = new Uri(url);
                string host = uri.Host;
                if (!Allowed.Contains(host) && KnownHosts.Contains(host))
                {
                    Console.Error.WriteLine("refused: the oracle does not run ac://" + host);
                    return 2;
                }
                if (host == "savetofile" && HttpUtility.ParseQueryString(uri.Query)["location"] == "appdata")
                {
                    Console.Error.WriteLine("refused: savetofile into the game's folder");
                    return 2;
                }
                string absolutePath = uri.AbsolutePath;
                NameValueCollection parameters = HttpUtility.ParseQueryString(uri.Query);
                if (body != null)
                    parameters.Add(HttpUtility.ParseQueryString(HttpUtility.UrlDecode(body)));
                object handler = Activator.CreateInstance(handlerType, true);
                var handlers = (IDictionary)handlerType.GetProperty("Handlers", BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance)?.GetValue(handler)
                    ?? (IDictionary)handlerType.GetField("Handlers", BindingFlags.Public | BindingFlags.NonPublic | BindingFlags.Instance).GetValue(handler);
                object data;
                if (handlers.Contains(host))
                {
                    try
                    {
                        data = ((Delegate)handlers[host]).DynamicInvoke(absolutePath, parameters);
                    }
                    catch (TargetInvocationException wrapped)
                    {
                        Exception ex = wrapped.InnerException;
                        if (ex is ArgumentOutOfRangeException)
                            data = message.Invoke(null, new object[] { "A name/value pair is required: ac://setsetting/<inifile>?[<section>/]<name>=<value>", true });
                        else if (ex is FileNotFoundException)
                            data = message.Invoke(null, new object[] { "No such file: " + ((FileNotFoundException)ex).FileName, false });
                        else
                            data = message.Invoke(null, new object[] { "ERROR\n" + ex.Message, false });
                    }
                }
                else
                {
                    data = Activator.CreateInstance(launcher.GetType("AC.Launcher.ACRequestData", true), new object[] { false });
                }
                Type dataType = data.GetType();
                string mime = (string)dataType.GetField("MimeType").GetValue(data);
                bool ok = (bool)dataType.GetField("ReturnValue").GetValue(data);
                var stream = (Stream)dataType.GetField("stream").GetValue(data);
                var bytes = new MemoryStream();
                stream.Position = 0;
                stream.CopyTo(bytes);
                Write(transcript, line, mime, ok, bytes.ToArray());
            }
        }
        var after = Snapshot(realDocs);
        if (!before.SequenceEqual(after))
        {
            Console.Error.WriteLine("ALARM: Documents\\Assetto Corsa changed during the run");
            return 3;
        }
        Console.WriteLine("ok: Documents\\Assetto Corsa unchanged (" + before.Count + " files)");
        return 0;
    }

    static void Write(Stream transcript, string request, string mime, bool ok, byte[] body)
    {
        byte[] head = Encoding.UTF8.GetBytes("== " + request + "\n" + mime + " " + (ok ? "true" : "false") + " " + body.Length + "\n");
        transcript.Write(head, 0, head.Length);
        transcript.Write(body, 0, body.Length);
        transcript.WriteByte((byte)'\n');
    }

    // Every file of the folder: its path, size and last write time.
    static List<string> Snapshot(string folder)
    {
        var files = new List<string>();
        if (!Directory.Exists(folder)) return files;
        foreach (string file in Directory.GetFiles(folder, "*", SearchOption.AllDirectories).OrderBy(f => f, StringComparer.Ordinal))
        {
            var info = new FileInfo(file);
            files.Add(file + "|" + info.Length + "|" + info.LastWriteTimeUtc.Ticks);
        }
        return files;
    }
}
