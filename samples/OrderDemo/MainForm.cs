using System;
using System.Drawing;
using System.Globalization;
using System.Linq;
using System.Threading;
using System.Windows.Forms;

namespace OrderDemo
{
    /// <summary>The order screen.</summary>
    public class MainForm : Form
    {
        private readonly Product[] menu =
        {
            new Product("Coffee", 450, 20),
            new Product("Tea", 380, 20),
            new Product("Cheesecake", 520, 2),
        };

        private Order order = new Order();
        private readonly TextBox customerBox;
        private readonly ComboBox productBox;
        private readonly TextBox quantityBox;
        private readonly ListBox linesBox;
        private readonly Label totalLabel;
        private readonly Label statusLabel;

        public MainForm()
        {
            Name = "mainForm";
            Text = "Order Demo";
            ClientSize = new Size(460, 330);
            StartPosition = FormStartPosition.Manual;
            Location = new Point(120, 120);
            Font = new Font("Segoe UI", 9.5f);

            Controls.Add(new Label { Text = "Customer", Left = 16, Top = 18, Width = 80 });
            customerBox = new TextBox { Name = "customerBox", Left = 100, Top = 15, Width = 200 };
            Controls.Add(customerBox);

            Controls.Add(new Label { Text = "Product", Left = 16, Top = 52, Width = 80 });
            productBox = new ComboBox { Name = "productBox", Left = 100, Top = 49, Width = 200, DropDownStyle = ComboBoxStyle.DropDownList };
            productBox.Items.AddRange(menu);
            productBox.SelectedIndex = 0;
            Controls.Add(productBox);

            quantityBox = new TextBox { Name = "quantityBox", Left = 310, Top = 49, Width = 50, Text = "1" };
            Controls.Add(quantityBox);

            var addButton = new Button { Name = "addButton", Text = "Add", Left = 370, Top = 47, Width = 74 };
            addButton.Click += (s, e) => AddLine();
            Controls.Add(addButton);

            linesBox = new ListBox { Name = "linesBox", Left = 16, Top = 86, Width = 428, Height = 130 };
            Controls.Add(linesBox);

            totalLabel = new Label { Name = "totalLabel", Left = 16, Top = 228, Width = 300, Font = new Font("Segoe UI", 11f, FontStyle.Bold), Text = "Total: 0" };
            Controls.Add(totalLabel);

            var placeButton = new Button { Name = "placeButton", Text = "Place order", Left = 330, Top = 224, Width = 114 };
            placeButton.Click += (s, e) => PlaceOrder();
            Controls.Add(placeButton);

            statusLabel = new Label { Name = "statusLabel", Left = 16, Top = 270, Width = 428, Height = 40, ForeColor = Color.DimGray };
            Controls.Add(statusLabel);
        }

        private void AddLine()
        {
            var product = (Product)productBox.SelectedItem;
            if (!int.TryParse(quantityBox.Text, out int quantity) || quantity < 1)
            {
                statusLabel.Text = "Enter a quantity of 1 or more.";
                return;
            }
            try
            {
                order.Add(product, quantity);
                linesBox.Items.Add(order.Lines.Last());
                ShowTotal();
                statusLabel.Text = "";
            }
            catch (OutOfStockException ex)
            {
                statusLabel.Text = ex.Message;
            }
        }

        private void ShowTotal()
        {
            decimal total = order.Total();
            int discounted = order.DiscountedLines().Count();
            totalLabel.Text = discounted > 0 ? $"Total: {total:N0} (bulk discount on {discounted})" : $"Total: {total:N0}";
        }

        private void PlaceOrder()
        {
            order.Customer = customerBox.Text.Trim();
            if (order.Lines.Count == 0)
            {
                statusLabel.Text = "Add a product first.";
                return;
            }
            int saved = OrderRepository.Save(order);
            statusLabel.Text = saved > 0
                ? $"Order for {order.Customer} saved."
                : $"Order for {order.Customer} ({order.Total():N0}) placed. The sample has no database, so it was not saved.";
            order = new Order();
            linesBox.Items.Clear();
            totalLabel.Text = "Total: 0";
        }
    }

    internal static class Program
    {
        [STAThread]
        private static void Main(string[] args)
        {
            // The screen is in English, so show the .NET messages (exceptions) in English too, whatever the Windows language.
            Thread.CurrentThread.CurrentUICulture = CultureInfo.GetCultureInfo("en-US");
            Application.EnableVisualStyles();
            Application.SetCompatibleTextRenderingDefault(false);
            Application.Run(new MainForm());
        }
    }
}
