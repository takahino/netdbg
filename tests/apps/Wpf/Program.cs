// WPF の GUI のスモーク試験 (MCP の ui_* のツール。UI Automation で読む) の対象。tests/apps/Gui (WinForms) と同じ画面を
// WPF で作る (コントロールの名前、文字、目印もそろえる)。XAML は使わず、コードで組む。行は `// @名前` の目印で探す。
// 引数 `--close-after=<秒>` (既定 60) で、試験が止まっても窓が残らないよう、自分で閉じる。
using System;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Media;
using System.Windows.Threading;

static class Program
{
    [STAThread]
    static int Main(string[] args)
    {
        var app = new Application();
        var root = new Canvas();
        var window = new Window { Name = "mainWindow", Title = "Smoke WPF", Left = 120, Top = 120, Width = 440, Height = 340, WindowStartupLocation = WindowStartupLocation.Manual, Content = root };
        var nameBox = new TextBox { Name = "nameBox", Width = 160 };
        var greetButton = new Button { Name = "greetButton", Content = "Greet", Width = 80 };
        var agreeBox = new CheckBox { Name = "agreeBox", Content = "Agree" };
        var colorBox = new ComboBox { Name = "colorBox", Width = 120 };
        foreach (string color in new[] { "Red", "Green", "Blue" }) colorBox.Items.Add(color);
        var resultLabel = new TextBlock { Name = "resultLabel", Width = 380, Text = "" };
        var aboutButton = new Button { Name = "aboutButton", Content = "About", Width = 80 };
        // 座標でのクリックの試験: 押された位置 (canvas の中の座標) を clickLabel に書く。Panel には UI Automation の
        // 要素が無いので、背景を塗った Label にする。
        var canvas = new Label { Name = "canvas", Width = 200, Height = 120, Background = Brushes.SteelBlue };
        var clickLabel = new TextBlock { Name = "clickLabel", Width = 170, Text = "" };

        greetButton.Click += (s, e) =>
        {
            string greeting = "Hello, " + nameBox.Text + (agreeBox.IsChecked == true ? "!" : "") + " " + colorBox.Text; // @greet
            resultLabel.Text = greeting.Trim();
        };
        canvas.MouseLeftButtonUp += (s, e) =>
        {
            Point p = e.GetPosition(canvas);
            clickLabel.Text = (int)p.X + "," + (int)p.Y; // @canvas-click
        };
        aboutButton.Click += (s, e) =>
        {
            var panel = new Canvas();
            var about = new Window { Name = "aboutWindow", Title = "About Smoke WPF", Width = 260, Height = 150, Owner = window, WindowStartupLocation = WindowStartupLocation.CenterOwner, ResizeMode = ResizeMode.NoResize, ShowInTaskbar = false, Content = panel };
            var closeButton = new Button { Name = "closeButton", Content = "Close", Width = 80, IsCancel = true };
            closeButton.Click += (_, __) => about.Close();
            Place(panel, new TextBlock { Name = "aboutLabel", Text = "Smoke WPF 1.0" }, 16, 16);
            Place(panel, closeButton, 80, 50);
            about.ShowDialog();
        };
        Place(root, nameBox, 16, 16);
        Place(root, greetButton, 190, 14);
        Place(root, aboutButton, 280, 14);
        Place(root, agreeBox, 16, 52);
        Place(root, colorBox, 130, 48);
        Place(root, resultLabel, 16, 84);
        Place(root, canvas, 16, 120);
        Place(root, clickLabel, 230, 120);

        int seconds = 60;
        foreach (string arg in args)
        {
            if (arg.StartsWith("--close-after=")) seconds = int.Parse(arg.Substring("--close-after=".Length));
        }
        var closer = new DispatcherTimer { Interval = TimeSpan.FromSeconds(seconds) };
        closer.Tick += (s, e) => app.Shutdown();
        closer.Start();
        app.Run(window);
        return 0;
    }

    static void Place(Canvas panel, UIElement element, double left, double top)
    {
        Canvas.SetLeft(element, left);
        Canvas.SetTop(element, top);
        panel.Children.Add(element);
    }
}
