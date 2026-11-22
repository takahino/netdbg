// GUI のスモーク試験 (MCP の ui_* のツール) の対象。行は `// @名前` の目印で探す。
// 引数 `--close-after=<秒>` (既定 60) で、試験が止まっても窓が残らないよう、自分で閉じる。
using System;
using System.Drawing;
using System.Windows.Forms;

static class Program
{
    [STAThread]
    static int Main(string[] args)
    {
        Application.EnableVisualStyles();
        var form = new Form { Name = "mainForm", Text = "Smoke GUI", ClientSize = new Size(420, 300), StartPosition = FormStartPosition.Manual, Location = new Point(120, 120) };
        var nameBox = new TextBox { Name = "nameBox", Left = 16, Top = 16, Width = 160 };
        var greetButton = new Button { Name = "greetButton", Text = "Greet", Left = 190, Top = 14, Width = 80 };
        var agreeBox = new CheckBox { Name = "agreeBox", Text = "Agree", Left = 16, Top = 50, Width = 100 };
        var colorBox = new ComboBox { Name = "colorBox", Left = 130, Top = 48, Width = 120, DropDownStyle = ComboBoxStyle.DropDownList };
        colorBox.Items.AddRange(new object[] { "Red", "Green", "Blue" });
        var resultLabel = new Label { Name = "resultLabel", Left = 16, Top = 84, Width = 380, Text = "" };
        var aboutButton = new Button { Name = "aboutButton", Text = "About", Left = 280, Top = 14, Width = 80 };
        // 座標でのクリックの試験: 押された位置 (canvas の中の座標) を clickLabel に書く。
        var canvas = new Panel { Name = "canvas", Left = 16, Top = 120, Width = 200, Height = 120, BackColor = Color.SteelBlue };
        var clickLabel = new Label { Name = "clickLabel", Left = 230, Top = 120, Width = 170, Text = "" };

        greetButton.Click += (s, e) =>
        {
            string greeting = "Hello, " + nameBox.Text + (agreeBox.Checked ? "!" : "") + " " + colorBox.Text; // @greet
            resultLabel.Text = greeting.Trim();
        };
        canvas.MouseClick += (s, e) =>
        {
            clickLabel.Text = e.X + "," + e.Y; // @canvas-click
        };
        aboutButton.Click += (s, e) =>
        {
            using (var about = new Form { Name = "aboutForm", Text = "About Smoke GUI", ClientSize = new Size(240, 100), StartPosition = FormStartPosition.CenterParent })
            {
                var closeButton = new Button { Name = "closeButton", Text = "Close", Left = 80, Top = 50, DialogResult = DialogResult.OK };
                about.Controls.Add(new Label { Name = "aboutLabel", Text = "Smoke GUI 1.0", Left = 16, Top = 16, Width = 200 });
                about.Controls.Add(closeButton);
                about.ShowDialog(form);
            }
        };
        form.Controls.AddRange(new Control[] { nameBox, greetButton, agreeBox, colorBox, resultLabel, aboutButton, canvas, clickLabel });

        int seconds = 60;
        foreach (string arg in args)
        {
            if (arg.StartsWith("--close-after=")) seconds = int.Parse(arg.Substring("--close-after=".Length));
        }
        var closer = new Timer { Interval = seconds * 1000 };
        closer.Tick += (s, e) => Application.Exit();
        closer.Start();
        Application.Run(form);
        return 0;
    }
}
